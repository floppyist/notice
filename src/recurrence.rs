use chrono::Datelike;

// Advance a YYYY-MM-DD date to its next occurrence for a repeat rule.
fn add_month_clamped(d: chrono::NaiveDate) -> Option<chrono::NaiveDate> {
    let (y, m) = if d.month() == 12 {
        (d.year() + 1, 1)
    } else {
        (d.year(), d.month() + 1)
    };
    let (ny, nm) = if m == 12 { (y + 1, 1) } else { (y, m + 1) };
    let last = chrono::NaiveDate::from_ymd_opt(ny, nm, 1)?.pred_opt()?.day();
    chrono::NaiveDate::from_ymd_opt(y, m, d.day().min(last))
}

pub fn advance_repeat_date(iso: &str, rule: &str) -> Option<String> {
    let d = chrono::NaiveDate::parse_from_str(iso, "%Y-%m-%d").ok()?;
    let next = match rule {
        "daily" => d.succ_opt(),
        "weekly" => d.checked_add_days(chrono::Days::new(7)),
        "monthly" => add_month_clamped(d),
        _ => None,
    };
    next.map(|nd| nd.format("%Y-%m-%d").to_string())
}

// Advance the start/end dates of every appointment inside the JSON-array string.
fn advance_appointments(json: &str, rule: &str) -> String {
    let fallback: String = json.to_string();
    let mut arr: Vec<serde_json::Value> = serde_json::from_str(json).unwrap_or_default();
    for apt in arr.iter_mut() {
        if let Some(obj) = apt.as_object_mut() {
            if let Some(s) = obj.get("start").and_then(|v| v.as_str()) {
                if let Some(ns) = advance_repeat_date(s, rule) {
                    if let Ok(v) = serde_json::from_str::<serde_json::Value>(&format!("\"{}\"", ns)) {
                        obj.insert("start".to_string(), v);
                    }
                }
            }
            if let Some(e) = obj.get("end").and_then(|v| v.as_str()) {
                if let Some(ne) = advance_repeat_date(e, rule) {
                    if let Ok(v) = serde_json::from_str::<serde_json::Value>(&format!("\"{}\"", ne)) {
                        obj.insert("end".to_string(), v);
                    }
                }
            }
        }
    }
    if arr.is_empty() {
        fallback
    } else {
        serde_json::to_string(&arr).unwrap_or(fallback)
    }
}

// Advance a note's recurrence once: due_date + appointment dates move forward
// and the note returns to the backlog.
pub fn apply_recurrence(
    rule: &str,
    due_date: Option<String>,
    appointments: Option<String>,
) -> (Option<String>, Option<String>) {
    let new_due = match due_date {
        Some(ref d) if !d.trim().is_empty() => advance_repeat_date(d, rule).or(due_date),
        _ => advance_repeat_date(
            &chrono::Local::now().format("%Y-%m-%d").to_string(),
            rule,
        ),
    };
    let new_appts = appointments.map(|a| advance_appointments(&a, rule));
    (new_due, new_appts)
}