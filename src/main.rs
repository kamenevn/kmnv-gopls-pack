// Панель пакета: одна кнопка «Version» — её ответ меняется между релизами,
// поэтому приёмка обновления видит новый пакет, а не прежний.
use std::io::{Read, Write};

fn main() {
    let mut input = String::new();
    std::io::stdin().read_to_string(&mut input).unwrap();
    let request: serde_json::Value =
        serde_json::from_str(&input).unwrap_or(serde_json::Value::Null);
    let action = request["action"].as_str().unwrap_or("").to_string();
    let reply = if action == "version" {
        serde_json::json!({
            "error": "",
            "text": format!("gopls-pack v{}", env!("CARGO_PKG_VERSION")),
        })
    } else {
        serde_json::json!({ "error": format!("unknown action: {action}") })
    };
    std::io::stdout().write_all(reply.to_string().as_bytes()).unwrap();
}
