#![cfg(not(target_os = "windows"))]
#![allow(clippy::expect_used, clippy::unwrap_used)]

use core_test_support::responses;
use core_test_support::test_codex_exec::test_codex_exec;
use predicates::str::is_empty;
use pretty_assertions::assert_eq;
use wiremock::matchers::any;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn outputs_only_final_message_with_last_flag() -> anyhow::Result<()> {
    let test = test_codex_exec();
    let server = responses::start_mock_server().await;
    let body = responses::sse(vec![
        responses::ev_response_created("resp1"),
        responses::ev_assistant_message("msg1", "final answer"),
        responses::ev_completed("resp1"),
    ]);
    responses::mount_sse_once_match(&server, any(), body).await;

    test.cmd_with_server(&server)
        .arg("--skip-git-repo-check")
        .arg("--last")
        .arg("say hello")
        .assert()
        .success()
        .stdout("final answer\n")
        .stderr(is_empty());

    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn writes_last_message_file_in_last_mode() -> anyhow::Result<()> {
    let test = test_codex_exec();
    let server = responses::start_mock_server().await;
    let body = responses::sse(vec![
        responses::ev_response_created("resp1"),
        responses::ev_assistant_message("msg1", "final answer"),
        responses::ev_completed("resp1"),
    ]);
    responses::mount_sse_once_match(&server, any(), body).await;

    let last_path = test.cwd_path().join("last.txt");

    test.cmd_with_server(&server)
        .arg("--skip-git-repo-check")
        .arg("--last")
        .arg("--output-last-message")
        .arg(&last_path)
        .arg("say hello")
        .assert()
        .success()
        .stdout("final answer\n")
        .stderr(is_empty());

    let saved = std::fs::read_to_string(&last_path)?;
    assert_eq!(saved, "final answer");

    Ok(())
}
