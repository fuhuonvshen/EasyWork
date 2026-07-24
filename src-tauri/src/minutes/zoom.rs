// EasyWork - Zoom launcher command

#[tauri::command]
pub fn launch_zoom(url: String) -> Result<(), String> {
    let target = if url.contains("zoommtg://") {
        url
    } else if url.contains("zoom.us") {
        let confno = url
            .split("/j/")
            .nth(1)
            .and_then(|s| s.split('?').next())
            .unwrap_or("");
        let pwd = url
            .split("pwd=")
            .nth(1)
            .unwrap_or("");
        if confno.is_empty() {
            url
        } else if pwd.is_empty() {
            format!("zoommtg://zoom.us/join?action=join&confno={}", confno)
        } else {
            format!("zoommtg://zoom.us/join?action=join&confno={}&pwd={}", confno, pwd)
        }
    } else {
        url
    };
    open::that(&target).map_err(|e| format!("无法启动 Zoom: {}", e))
}
