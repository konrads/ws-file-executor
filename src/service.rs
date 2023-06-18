use crate::defer;
use async_trait::async_trait;
use std::{collections::HashMap, fs::File, io, path::Path};
use tokio::{
    io::{AsyncBufReadExt, BufReader},
    process::Command,
    sync::{mpsc::Sender, Mutex},
};

///
#[derive(thiserror::Error, Debug)]
pub enum ServiceError {
    #[error("failed to spawn")]
    SpawnError(#[from] io::Error),
    #[error("failed to execute, status code {0}")]
    ExecutionError(i32),
    #[error("failed to obtain status code")]
    NoStatusCode,
    #[error("failed to find uploaded file {id}")]
    UploadedFileNotFound { id: String },
    #[error("failed to stream")]
    StreamError,
    #[error("failed to save file {id}, {path}")]
    FileSaveError { id: String, path: String },
}

/// Services trait allows for mocking
#[async_trait]
pub(crate) trait Services: Send + Sync {
    async fn register_file(
        &self,
        id: &str,
        file_path: &str,
        source_file: &mut File,
    ) -> Result<(), ServiceError>;
    async fn run_cmd(
        &self,
        id: &str,
        cmd: &str,
        stdout_sender: Sender<String>,
    ) -> Result<(), ServiceError>;
}

///
pub(crate) struct ProdServices {
    uploads_dir: String,
    files: Mutex<HashMap<String, String>>,
}

impl ProdServices {
    pub fn new(uploads_dir: String) -> Self {
        Self {
            uploads_dir,
            files: Mutex::new(HashMap::new()),
        }
    }

    /// Helper
    #[cfg(test)]
    pub fn with_init_files(uploads_dir: String, files: HashMap<String, String>) -> Self {
        Self {
            uploads_dir,
            files: Mutex::new(files),
        }
    }

    /// Helper function for test purposes only.
    #[cfg(test)]
    pub async fn get_file(&self, id: &str) -> Option<String> {
        self.files.lock().await.get(id).cloned()
    }
}

#[async_trait]
impl Services for ProdServices {
    /// Register a file for later execution. Registration consists of persisting and recording it's path under generated id.
    async fn register_file(
        &self,
        id: &str,
        file_path: &str,
        source_file: &mut File,
    ) -> Result<(), ServiceError> {
        let full_path = format!("{}/{}/{}", self.uploads_dir, id, file_path);
        let parent_path = Path::new(&full_path)
            .parent()
            .ok_or(ServiceError::FileSaveError {
                id: id.to_owned(),
                path: full_path.to_owned(),
            })?;

        std::fs::create_dir_all(parent_path).map_err(|_| ServiceError::FileSaveError {
            id: id.to_owned(),
            path: full_path.to_owned(),
        })?;
        let mut writer = &File::create(&full_path).map_err(|_| ServiceError::FileSaveError {
            id: id.to_owned(),
            path: full_path.to_owned(),
        })?;
        io::copy(source_file, &mut writer).map_err(|_| ServiceError::FileSaveError {
            id: id.to_owned(),
            path: full_path.to_owned(),
        })?;

        self.files.lock().await.insert(id.to_string(), full_path);

        Ok(())
    }

    /// Execute a command on a registered file. The command is executed in a separate process and the stdout is streamed back to the caller.
    async fn run_cmd(
        &self,
        id: &str,
        cmd: &str,
        stdout_sender: Sender<String>,
    ) -> Result<(), ServiceError> {
        // declare cleanup to be done upon function exit
        defer! {
            let base_path = format!("{}/{}", self.uploads_dir, id);
            std::fs::remove_dir_all(&base_path)
                .unwrap_or_else(|_| log::error!("Failed to clean up path: {base_path}"));
            log::info!("Cleaned up file id: {id}, path: {base_path}");
        }

        let registered_file = self.files.lock().await.remove(id);

        if let Some(file_path) = registered_file {
            let mut command = Command::new(cmd)
                .arg(file_path)
                .stdout(std::process::Stdio::piped())
                .spawn()
                .map_err(ServiceError::SpawnError)?;

            if let Some(stdout) = &mut command.stdout {
                let mut lines = BufReader::new(stdout).lines();
                while let Ok(Some(line)) = lines.next_line().await {
                    stdout_sender
                        .send(line)
                        .await
                        .map_err(|_| ServiceError::StreamError)?;
                }
            }

            match command.wait().await? {
                status if status.success() => Ok(()),
                status => Err(status
                    .code()
                    .map_or_else(|| ServiceError::NoStatusCode, ServiceError::ExecutionError)),
            }
        } else {
            Err(ServiceError::UploadedFileNotFound { id: id.to_string() })
        }
    }
}
