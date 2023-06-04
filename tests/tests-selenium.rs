use std::time::Duration;

use futures::future::join_all;
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::{process::Command, time::sleep};

const PARALLEL_TESTS: u32 = 30;

#[cfg_attr(not(feature = "integration-test"), ignore)]
#[actix_rt::test]
async fn test_selenium() {
    // start the server
    let server = Command::new("target/debug/ws-file-executor").spawn();
    // ensure server is up
    sleep(Duration::from_secs(5)).await;

    // run in parallel, rountrip1..4 tests
    let roundtrips = join_all((0..PARALLEL_TESTS).map(|x| run_roundtrips(x))).await;
    let failed_roundrips = roundtrips
        .into_iter()
        .filter(|(_, _, _, success)| !success)
        .collect::<Vec<_>>();

    // kill the server
    server.unwrap().kill().await.unwrap();

    // validate all roundtrips succeeded
    assert_eq!(Vec::<(u32, u64, String, bool)>::new(), failed_roundrips);
}

#[cfg_attr(not(feature = "integration-test"), ignore)]
async fn run_roundtrips(x: u32) -> (u32, u64, String, bool) {
    let start = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs();
    for j in 1..=4 {
        let sh_script = format!("roundtrip{j}.sh");
        let roundtrip_res = Command::new("python")
            .arg(format!("tests/roundtrip-test.py"))
            .arg(&sh_script)
            .spawn();
        let success = roundtrip_res
            .expect(&format!("sh script {sh_script} failed to start"))
            .wait()
            .await
            .map_or(false, |status| status.success());
        if !success {
            let end = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_secs();
            return (x, end - start, sh_script, false);
        }
    }
    let end = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs();

    (x, end - start, "all".to_owned(), true)
}
