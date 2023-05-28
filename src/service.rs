use async_trait::async_trait;
use std::{collections::HashMap, fs::File, io, path::Path};
use tokio::{
    io::{AsyncBufReadExt, BufReader},
    process::Command,
    sync::{mpsc::Sender, Mutex, RwLock},
};

///
#[derive(thiserror::Error, Debug)]
pub enum ServiceError {
    #[error("failed to spawn")]
    SpawnError(#[from] io::Error),
    #[error("failed to execute, status code {0}")]
    ExecutionError(i32),
    #[error("failed to find uploaded file {id}")]
    UploadedFileNotFound { id: String },
    #[error("failed to stream")]
    StreamError,
    #[error("failed to cleanup {id}")]
    CleanupError { id: String },
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
    files: RwLock<HashMap<String, Mutex<String>>>,
}

impl ProdServices {
    pub fn new(uploads_dir: String) -> Self {
        Self {
            uploads_dir,
            files: RwLock::new(HashMap::new()),
        }
    }

    /// Helper
    #[cfg(test)]
    pub fn with_init_files(uploads_dir: String, files: HashMap<String, String>) -> Self {
        Self {
            uploads_dir,
            files: RwLock::new(files.into_iter().map(|(k, v)| (k, Mutex::new(v))).collect()),
        }
    }

    /// Helper function for test purposes only. Unlocks the files map entries via read locks.
    #[cfg(test)]
    pub async fn get_file(&self, id: &str) -> Option<String> {
        let files = self.files.read().await;
        if let Some(file_entry) = files.get(id).clone() {
            let file_path = file_entry.lock().await.clone();
            Some(file_path)
        } else {
            None
        }
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

        self.files
            .write()
            .await
            .insert(id.to_string(), Mutex::new(full_path.to_string()));

        Ok(())
    }

    /// Execute a command on a registered file. The command is executed in a separate process and the stdout is streamed back to the caller.
    /// Note: cleanup async closure does primitive "finally" cleanup.
    async fn run_cmd(
        &self,
        id: &str,
        cmd: &str,
        stdout_sender: Sender<String>,
    ) -> Result<(), ServiceError> {
        let files = self.files.read().await;

        let cleanup = async {
            let base_path = format!("{}/{}", self.uploads_dir, id);
            std::fs::remove_dir_all(&base_path)
                .map_err(|_| ServiceError::CleanupError { id: id.to_owned() })?;
            self.files.write().await.remove(id);
            log::info!("Cleaned up file id: {id}, path: {base_path}");
            Ok::<(), ServiceError>(())
        };

        if let Some(file_entry) = files.get(id) {
            let file_path = file_entry.lock().await.clone();
            drop(files);
            let mut command = match Command::new(cmd)
                .arg(file_path)
                .stdout(std::process::Stdio::piped())
                .spawn()
            {
                Ok(cmd) => cmd,
                Err(e) => {
                    cleanup.await.ok();
                    return Err(ServiceError::SpawnError(e));
                }
            };

            if let Some(stdout) = &mut command.stdout {
                let mut lines = BufReader::new(stdout).lines();
                while let Ok(Some(line)) = lines.next_line().await {
                    match stdout_sender.send(line).await {
                        Ok(_) => (),
                        Err(_) => {
                            cleanup.await.ok();
                            return Err(ServiceError::StreamError);
                        }
                    }
                }
            }

            cleanup.await?;
            match command.wait().await? {
                status if status.success() => Ok(()),
                status => Err(ServiceError::ExecutionError(status.code().unwrap_or(-1))),
            }
        } else {
            Err(ServiceError::UploadedFileNotFound { id: id.to_string() })
        }
    }
}
