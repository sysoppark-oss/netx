fn main() {
    // 화면이 호출할 수 있는 백엔드 명령 목록.
    // 여기에 등록하고 capabilities/default.json에서 허용한 명령만 화면에서 호출할 수 있다.
    tauri_build::try_build(
        tauri_build::Attributes::new().app_manifest(
            tauri_build::AppManifest::new().commands(&["app_info", "environment_status"]),
        ),
    )
    .expect("failed to run tauri-build");
}
