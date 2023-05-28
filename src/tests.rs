use super::*;
use actix_web::{http::StatusCode, web, App};
use async_trait::async_trait;
use mockall::{mock, predicate::*};
use service::ServiceError;
use std::fs::File;
use std::sync::Arc;
use tokio::sync::mpsc::Sender;

// Mock of the Services trait, allows for better route availability testing
mock! {
    pub TestServices {}
    impl Clone for TestServices {
        fn clone(&self) -> Self;
    }
    unsafe impl Send for TestServices {}
    unsafe impl Sync for TestServices {}
    #[async_trait]
    impl Services for TestServices {
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
}

#[actix_rt::test]
async fn test_routes() {
    let mut services = MockTestServices::new();
    services
        .expect_run_cmd()
        .times(1)
        .returning(|_, _, _| Ok(()));
    let services_arc: Arc<dyn Services> = Arc::new(services);

    let server = actix_test::start(move || init_app_routes!(services_arc));

    // validate NOT_FOUND on bogus endpoint
    let res = server.get("/bogus-endpoint").send().await.unwrap();
    assert_eq!(res.status(), StatusCode::NOT_FOUND);

    // validate / serves index.html
    let mut res = server.get("/").send().await.unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let body_bytes = res.body().await.unwrap();
    let body = std::str::from_utf8(&body_bytes).unwrap();
    assert_eq!(body, include_str!("../static/index.html"));

    // validate existence of /execute-cmd-ws - note expect BAD_REQUEST (NOT_FOUND!) due to lack of ws upgrade
    let res = server.get("/runCommand").send().await.unwrap();
    assert_eq!(res.status(), StatusCode::BAD_REQUEST);

    // validate existence of /upload - note expect BAD_REQUEST (NOT_FOUND!) due to lack of multipart/form-data
    let res = server.post("/upload").send().await.unwrap();
    assert_eq!(res.status(), StatusCode::BAD_REQUEST);
}
