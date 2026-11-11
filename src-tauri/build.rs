fn main() {
    // 자동 업데이트용: 어느 채널(main, dev)의 어느 커밋으로 언제 빌드했는지 박아 넣는다.
    // GitHub Actions에서 빌드할 때만 값이 있고, 직접 빌드하면 비어 있어 업데이트를 하지 않는다.
    for key in ["GITHUB_SHA", "GITHUB_REF_NAME", "GITHUB_EVENT_NAME"] {
        println!("cargo:rerun-if-env-changed={key}");
    }
    let pushed = std::env::var("GITHUB_EVENT_NAME").is_ok_and(|e| e == "push");
    let env = |key: &str| std::env::var(key).ok().filter(|_| pushed).unwrap_or_default();
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
    println!("cargo:rustc-env=NETX_BUILD_COMMIT={}", env("GITHUB_SHA"));
    println!("cargo:rustc-env=NETX_BUILD_CHANNEL={}", env("GITHUB_REF_NAME"));
    println!("cargo:rustc-env=NETX_BUILD_UNIX={now}");

    // 화면이 호출할 수 있는 백엔드 명령 목록.
    // 여기에 등록하고 capabilities/default.json에서 허용한 명령만 화면에서 호출할 수 있다.
    tauri_build::try_build(
        tauri_build::Attributes::new().app_manifest(
            tauri_build::AppManifest::new().commands(&[
                "app_info",
                "environment_status",
                "get_tools",
                "get_recipes",
                "plan_from_recipe",
                "validate_plan",
                "run_recipe",
                "cancel_run",
                "tool_status",
                "install_tools",
                "run_ai_plan",
                "suggest",
                "brief_run",
                "get_settings",
                "save_settings",
                "test_llm",
                "list_interfaces",
                "network_info",
                "check_command",
                "plan_mixed",
                "run_mixed",
                "open_target",
                "kill_process",
                "save_topology",
                "apply_update",
                "get_word",
                "open_church_mail",
                "capture_list",
                "capture_open",
                "capture_rows",
                "capture_packet",
                "capture_folder",
                "voice_typing",
                "bridge_take_handoff",
                "bridge_status",
                "open_terminal",
                "term_take_preset",
                "term_ports",
                "term_open",
                "term_write",
                "term_resize",
                "term_close",
                "term_folder",
            ]),
        ),
    )
    .expect("failed to run tauri-build");
}
