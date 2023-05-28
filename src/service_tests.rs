use crate::service::*;
use actix_web::rt;
use futures::future::join_all;
use std::sync::Arc;
use std::{collections::HashMap, fs::File, path::Path};
use std::{fs, time::SystemTime};
use tokio::sync::mpsc;

#[actix_rt::test]
async fn test_register_file() {
    let services = ProdServices::new("test-stage/uploads".to_owned());
    let id = ".id123";
    let uploaded_path = "ignore/me";
    let source_file_path = "test-stage/scripts/register-file.txt";
    let mut source_file = File::open(source_file_path).unwrap();
    services
        .register_file(id, uploaded_path, &mut source_file)
        .await
        .unwrap();
    let file = services.get_file(id).await.unwrap();
    assert_eq!(file, format!("test-stage/uploads/{id}/{uploaded_path}"));

    // read source_file and compare to uploads/.gitkeep
    assert_eq!(
        "register-file contents",
        read_file(&format!("test-stage/uploads/{id}/{uploaded_path}"))
    );
}

#[actix_rt::test]
async fn test_execute_cmd() {
    write_file(
        "test-stage/uploads/.id345/mock-path/mock-script.sh",
        "echo Sleep for 2\nsleep 2\n echo Done",
    );

    let services = ProdServices::with_init_files(
        "test-stage/uploads".to_owned(),
        HashMap::from([(
            ".id345".to_owned(),
            "test-stage/uploads/.id345/mock-path/mock-script.sh".to_owned(),
        )]),
    );

    let (sender, mut receiver) = mpsc::channel::<String>(1024);

    let t0 = SystemTime::now();
    services.run_cmd(".id345", "sh", sender).await.unwrap();

    let mut output = String::new();
    while let Some(line) = receiver.recv().await {
        output.push_str(&format!("{line}\n"));
    }
    let t1 = SystemTime::now();

    assert_eq!(output, "Sleep for 2\nDone\n");
    let t_delta = t1.duration_since(t0).unwrap().as_millis();
    dbg!(t_delta);
    assert!(t_delta >= 2000 && t_delta < 3000);
}

#[actix_rt::test]
async fn test_execute_bogus_cmd() {
    let services = ProdServices::with_init_files(
        "test-stage/uploads".to_owned(),
        HashMap::from([(".id456".to_owned(), "bogus_file".to_owned())]),
    );

    let (sender, _receiver) = mpsc::channel::<String>(1024);

    let res = services.run_cmd(".id456", "bogus-cmd", sender).await;
    match res {
        Err(ServiceError::SpawnError(_)) => (),
        _ => panic!("Expected SpawnError"),
    }
}

#[actix_rt::test]
async fn test_execute_on_bogus_file() {
    write_file("test-stage/uploads/.id567/evil/die-script.sh", "exit 66");

    let services = ProdServices::with_init_files(
        "test-stage/uploads".to_owned(),
        HashMap::from([(".id567".to_owned(), "bogus_file".to_owned())]),
    );

    let (sender, _receiver) = mpsc::channel::<String>(1024);

    let res = services
        .run_cmd(
            ".id456",
            "test-stage/uploads/.id567/evil/die-script.sh",
            sender,
        )
        .await;
    match res {
        Err(ServiceError::UploadedFileNotFound { id }) if id == ".id456".to_owned() => (),
        _ => panic!("Expected SpawnError"),
    }
}

/// Tests parallel roundtrips, mimicking multiple browser sessions
#[actix_rt::test]
async fn test_parallel_roundtrips() {
    let services = Arc::new(ProdServices::new("test-stage/uploads".to_owned()));

    let mut scenario_futures = vec![];
    for i in 0..100 {
        let i: u32 = i % 4 + 1;
        let services = services.clone();

        let future = rt::spawn(async move {
            let id = uuid::Uuid::new_v4().to_string();
            let mut source_file =
                File::open(format!("test-stage/scripts/roundtrip{i}.sh")).unwrap();

            services
                .register_file(&id, "scripts", &mut source_file)
                .await
                .unwrap();

            let (sender, mut receiver) = mpsc::channel::<String>(1024);

            let t0 = SystemTime::now();
            services.run_cmd(&id, "sh", sender).await.unwrap();

            let mut output = String::new();
            while let Some(line) = receiver.recv().await {
                output.push_str(&format!("{line}\n"));
            }
            let t1 = SystemTime::now();
            let t_delta = t1.duration_since(t0).unwrap().as_millis();

            (i, t_delta, output)
        });

        scenario_futures.push(future);
    }
    let all_results = join_all(scenario_futures).await;
    for result in all_results {
        assert!(matches!(result, Ok((_, _, _))));
        let (i, t_delta, output) = result.unwrap();
        let exp_output = match i {
            1 => "1.1 Sleep for 1\n1.2 Sleep for 1\n1.3 Sleep for 1\n1.4 Done\n",
            2 => "2.1 Sleep for 2\n2.2 Sleep for 1\n2.3 Done\n",
            3 => "3.1 Sleep for 1\n3.2 Sleep for 2\n3.3 Done\n",
            4 => "4.1 Sleep for 3\n4.2 Done\n",
            _ => panic!("Unexpected i: {}", i),
        };
        dbg!(t_delta);
        assert!(t_delta >= 3000 && t_delta < 4000);
        assert_eq!(exp_output, output);
    }
}

// helpers
fn write_file(path: &str, contents: &str) {
    let parent_path = Path::new(path).parent().unwrap();
    std::fs::create_dir_all(parent_path).unwrap();
    fs::write(path, contents).unwrap();
}

fn read_file(path: &str) -> String {
    fs::read_to_string(path).unwrap()
}
