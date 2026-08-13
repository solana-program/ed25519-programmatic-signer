use serde_json::Value;

pub(crate) fn format_simulation(simulation: &Value) -> String {
    let value = simulation.get("value").unwrap_or(simulation);
    let err = value.get("err").cloned().unwrap_or(Value::Null);
    let units = value.get("unitsConsumed").cloned().unwrap_or(Value::Null);
    let mut lines = vec![format!("err: {err}"), format!("units consumed: {units}")];
    if let Some(logs) = value.get("logs").and_then(Value::as_array) {
        lines.push(String::from("logs:"));
        for log in logs {
            if let Some(log) = log.as_str() {
                lines.push(format!("  {log}"));
            }
        }
    }
    lines.join("\n")
}
