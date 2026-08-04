use std::io;
use std::path::Path;

use serde_json::{Map, Value};

use super::command::hook_command;
use super::config_edit::hook_command_variants;

/// reasonix hook events mapped to herdr agent states. Mirrors the kimi event
/// table: gating events map to blocked/working, lifecycle events to
/// session/idle. `Notification` fires when the agent needs the user's
/// attention (e.g. a pending approval), so it maps to blocked.
const REASONIX_HOOK_EVENTS: [(&str, &str); 12] = [
    ("SessionStart", "session"),
    ("UserPromptSubmit", "working"),
    ("PreToolUse", "working"),
    ("PostToolUse", "working"),
    ("PostToolUseFailure", "working"),
    ("SubagentStop", "working"),
    ("PreCompact", "working"),
    ("PermissionRequest", "blocked"),
    ("Notification", "blocked"),
    ("Stop", "idle"),
    ("StopFailure", "idle"),
    ("SessionEnd", "idle"),
];

/// Install herdr's state-reporting hooks into reasonix's global settings.json
/// (`<Reasonix home>/settings.json`). reasonix parses this file with Go's
/// encoding/json, so it is strict JSON; user-owned hooks are preserved and
/// only herdr-owned entries are added.
pub(crate) fn install(content: &str, settings_path: &Path, hook_path: &Path) -> io::Result<String> {
    let desired = apply_hooks(parse_value(content, settings_path)?, hook_path, true)?;
    write_if_changed(content, settings_path, desired)
}

/// Remove herdr's state-reporting hooks from reasonix's settings.json while
/// preserving user-owned hooks.
pub(crate) fn uninstall(
    content: &str,
    settings_path: &Path,
    hook_path: &Path,
) -> io::Result<String> {
    let desired = apply_hooks(parse_value(content, settings_path)?, hook_path, false)?;
    write_if_changed(content, settings_path, desired)
}

fn apply_hooks(
    mut root: Value,
    hook_path: &Path,
    installing: bool,
) -> io::Result<Value> {
    let root_object = root.as_object_mut().ok_or_else(|| {
        io::Error::other("reasonix settings must be a JSON object")
    })?;
    let hooks = root_object
        .entry("hooks".to_string())
        .or_insert_with(|| Value::Object(Map::new()))
        .as_object_mut()
        .ok_or_else(|| io::Error::other("reasonix settings hooks must be a JSON object"))?;

    for (event, action) in REASONIX_HOOK_EVENTS {
        let command = hook_command(hook_path, Some(action));
        let entries = hooks
            .entry(event.to_string())
            .or_insert_with(|| Value::Array(Vec::new()))
            .as_array_mut()
            .ok_or_else(|| {
                io::Error::other(format!("reasonix hook entries for {event} must be an array"))
            })?;

        if installing {
            if !entries.iter().any(|entry| {
                entry.get("command").and_then(Value::as_str) == Some(command.as_str())
            }) {
                entries.push(Value::Object(Map::from_iter([
                    ("command".to_string(), Value::String(command)),
                    ("timeout".to_string(), Value::Number(5000.into())),
                ])));
            }
        } else {
            let commands = hook_command_variants(hook_path, Some(action));
            entries.retain(|entry| {
                !entry
                    .get("command")
                    .and_then(Value::as_str)
                    .is_some_and(|candidate| commands.iter().any(|known| known == candidate))
            });
            if entries.is_empty() {
                hooks.remove(event);
            }
        }
    }

    if hooks.is_empty() {
        root_object.remove("hooks");
    }

    Ok(root)
}

/// Exposes the event table to integration tests.
#[cfg(test)]
pub(crate) fn hook_events_for_test() -> &'static [(&'static str, &'static str)] {
    &REASONIX_HOOK_EVENTS
}

fn parse_value(content: &str, settings_path: &Path) -> io::Result<Value> {    serde_json::from_str(content).map_err(|err| {
        io::Error::other(format!(
            "failed to parse reasonix settings at {}: {err}",
            settings_path.display()
        ))
    })
}

fn write_if_changed(
    original: &str,
    settings_path: &Path,
    desired: Value,
) -> io::Result<String> {
    let updated = format!(
        "{}\n",
        serde_json::to_string_pretty(&desired).map_err(|err| {
            io::Error::other(format!(
                "failed to serialize reasonix settings at {}: {err}",
                settings_path.display()
            ))
        })?
    );
    if updated.trim_end() == original.trim_end() {
        return Ok(original.to_string());
    }
    Ok(updated)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hook_path() -> std::path::PathBuf {
        #[cfg(windows)]
        {
            std::path::PathBuf::from("C:\\reasonix\\herdr-agent-state.ps1")
        }
        #[cfg(not(windows))]
        {
            std::path::PathBuf::from("/tmp/herdr-agent-state.sh")
        }
    }

    #[test]
    fn install_preserves_user_hooks_and_adds_herdr_entries() {
        let content = r#"{"hooks":{"PreToolUse":[{"command":"echo user","timeout":1000}]}}"#;
        let updated = install(content, Path::new("settings.json"), &hook_path()).unwrap();

        let value: Value = serde_json::from_str(&updated).unwrap();
        let hooks = value["hooks"].as_object().unwrap();
        let pre_tool = hooks["PreToolUse"].as_array().unwrap();
        assert_eq!(pre_tool.len(), 2);
        assert!(pre_tool.iter().any(|entry| entry["command"] == "echo user"));
        assert!(pre_tool.iter().any(|entry| {
            let command = entry["command"].as_str().unwrap();
            command.contains("herdr-agent-state.sh") && command.contains("working")
        }));
        // herdr entries carry a 5000ms timeout
        assert!(pre_tool.iter().any(|entry| entry["timeout"] == 5000));
        // all events present
        for (event, _) in REASONIX_HOOK_EVENTS {
            assert!(
                hooks[event].as_array().is_some(),
                "missing event {event}"
            );
        }
    }

    #[test]
    fn install_is_idempotent() {
        let content = "{}";
        let once = install(content, Path::new("settings.json"), &hook_path()).unwrap();
        let twice = install(&once, Path::new("settings.json"), &hook_path()).unwrap();
        assert_eq!(once, twice);
    }

    #[test]
    fn install_preserves_non_hook_top_level_keys() {
        let content = r#"{"telemetry":"auto"}"#;
        let updated = install(content, Path::new("settings.json"), &hook_path()).unwrap();
        let value: Value = serde_json::from_str(&updated).unwrap();
        assert_eq!(value["telemetry"], "auto");
        assert!(value["hooks"].is_object());
    }

    #[test]
    fn uninstall_removes_only_herdr_entries() {
        let content = r#"{"hooks":{"PreToolUse":[{"command":"echo user"},{"command":"bash '/tmp/herdr-agent-state.sh' working","timeout":5000}]}}"#;
        let updated = uninstall(content, Path::new("settings.json"), &hook_path()).unwrap();
        let value: Value = serde_json::from_str(&updated).unwrap();
        let hooks = value["hooks"].as_object().unwrap();
        let pre_tool = hooks["PreToolUse"].as_array().unwrap();
        assert_eq!(pre_tool.len(), 1);
        assert_eq!(pre_tool[0]["command"], "echo user");
        // empty events are dropped
        assert!(!hooks.contains_key("SessionStart"));
    }

    #[test]
    fn uninstall_drops_empty_hooks_object() {
        let content = r#"{"hooks":{"PreToolUse":[{"command":"bash '/tmp/herdr-agent-state.sh' working"}]}}"#;
        let updated = uninstall(content, Path::new("settings.json"), &hook_path()).unwrap();
        let value: Value = serde_json::from_str(&updated).unwrap();
        assert!(!value.as_object().unwrap().contains_key("hooks"));
    }

    #[test]
    fn rejects_non_object_root() {
        let result = install("[1,2]", Path::new("settings.json"), &hook_path());
        assert!(result.is_err());
    }
}
