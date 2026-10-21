//! Pressed input bookkeeping belongs to the controlled transport, including cancelled multi-step actions.
use serde_json::{json, Value};
use std::{
    collections::{BTreeMap, HashMap},
    sync::{Arc, Mutex},
};
use talos_error::{TalosError, TalosResult};

struct HeldInput {
    sequence: u64,
    release: Value,
}
#[derive(Default)]
struct State {
    sequence: u64,
    targets: BTreeMap<String, Vec<HeldInput>>,
}
/// A successful CDP acknowledgement may retire only these exact old holds.
/// A delayed response cannot retire a newer press of the same key or button.
pub(crate) struct PendingRelease {
    target: String,
    sequences: Vec<u64>,
}
#[derive(Clone, Default)]
pub(crate) struct PressedInputs(Arc<Mutex<State>>);
impl PressedInputs {
    /// Record down before send; keep up uncertain until a successful CDP reply.
    pub fn before_send(
        &self,
        command: &Value,
        sessions: &HashMap<String, String>,
    ) -> TalosResult<Option<PendingRelease>> {
        let Some(target) = command
            .get("sessionId")
            .and_then(Value::as_str)
            .and_then(|session| sessions.get(session))
        else {
            return Ok(None);
        };
        let Some(params) = command.get("params") else {
            return Ok(None);
        };
        let method = command
            .get("method")
            .and_then(Value::as_str)
            .unwrap_or_default();
        let event = params
            .get("type")
            .and_then(Value::as_str)
            .unwrap_or_default();
        let key = method == "Input.dispatchKeyEvent";
        let mouse = method == "Input.dispatchMouseEvent";
        let down =
            key && matches!(event, "keyDown" | "rawKeyDown") || mouse && event == "mousePressed";
        let up = key && event == "keyUp" || mouse && event == "mouseReleased";
        if !down && !up {
            return Ok(None);
        }
        let mut state = self.0.lock().map_err(|_| poisoned())?;
        let same = |entry: &HeldInput| {
            if key {
                same_key(&entry.release, params)
            } else {
                entry.release.get("button") == params.get("button")
            }
        };
        if up {
            let sequences = state
                .targets
                .get(target)
                .into_iter()
                .flatten()
                .filter(|entry| same(entry))
                .map(|entry| entry.sequence)
                .collect();
            return Ok(Some(PendingRelease {
                target: target.clone(),
                sequences,
            }));
        }
        state.sequence = state.sequence.checked_add(1).ok_or_else(poisoned)?;
        let sequence = state.sequence;
        let values = state.targets.entry(target.clone()).or_default();
        values.retain(|entry| !same(entry));
        if values.len() >= 256 {
            return Err(TalosError::new(
                talos_error::ErrorCode::Conflict,
                "Browser held-input capacity exceeded. Pause before continuing.",
            ));
        }
        let release = if key {
            let mut release = json!({"type":"keyUp","modifiers":0});
            for name in [
                "key",
                "code",
                "windowsVirtualKeyCode",
                "nativeVirtualKeyCode",
            ] {
                if let Some(value) = params.get(name) {
                    release[name] = value.clone();
                }
            }
            release
        } else {
            json!({"type":"mouseReleased","x":params.get("x").cloned().unwrap_or(json!(0)),"y":params.get("y").cloned().unwrap_or(json!(0)),"button":params.get("button").cloned().unwrap_or(Value::Null),"buttons":0,"clickCount":1})
        };
        values.push(HeldInput { sequence, release });
        Ok(None)
    }
    pub fn confirm_release(&self, release: PendingRelease) -> TalosResult<()> {
        let mut state = self.0.lock().map_err(|_| poisoned())?;
        if let Some(values) = state.targets.get_mut(&release.target) {
            values.retain(|entry| !release.sequences.contains(&entry.sequence));
            if values.is_empty() {
                state.targets.remove(&release.target);
            }
        }
        Ok(())
    }
    pub fn snapshot(&self) -> TalosResult<BTreeMap<String, Vec<Value>>> {
        Ok(self
            .0
            .lock()
            .map_err(|_| poisoned())?
            .targets
            .iter()
            .map(|(target, values)| {
                (
                    target.clone(),
                    values.iter().map(|entry| entry.release.clone()).collect(),
                )
            })
            .collect())
    }
    pub fn clear(&self) -> TalosResult<()> {
        self.0.lock().map_err(|_| poisoned())?.targets.clear();
        Ok(())
    }
}
fn poisoned() -> TalosError {
    TalosError::internal("Browser pressed-input state is poisoned.")
}
fn same_key(a: &Value, b: &Value) -> bool {
    [
        "code",
        "key",
        "windowsVirtualKeyCode",
        "nativeVirtualKeyCode",
    ]
    .into_iter()
    .find_map(|name| match (a.get(name), b.get(name)) {
        (Some(Value::String(a)), Some(Value::String(b))) if !a.is_empty() && !b.is_empty() => {
            Some(a == b)
        }
        (Some(Value::Number(a)), Some(Value::Number(b)))
            if a.as_u64() != Some(0) && b.as_u64() != Some(0) =>
        {
            Some(a == b)
        }
        _ => None,
    })
    .unwrap_or(false)
}
#[cfg(test)]
mod tests {
    use super::*;
    fn command(mouse: bool, event: &str) -> Value {
        json!({"method":if mouse {"Input.dispatchMouseEvent"} else {"Input.dispatchKeyEvent"},
            "sessionId":"session","params":{"type":event,"code":"","key":"Shift","button":"left","x":5,"y":6}})
    }
    #[test]
    fn repeated_key_and_mouse_down_have_one_release_and_empty_codes_use_the_key() {
        let pressed = PressedInputs::default();
        let sessions = HashMap::from([("session".into(), "target".into())]);
        for _ in 0..300 {
            for key in ["Shift", "Control"] {
                let _ = pressed.before_send(&json!({"method":"Input.dispatchKeyEvent","sessionId":"session","params":{"type":"rawKeyDown","code":"","key":key}}), &sessions).unwrap();
            }
            let _ = pressed
                .before_send(&command(true, "mousePressed"), &sessions)
                .unwrap();
        }
        assert_eq!(pressed.snapshot().unwrap()["target"].len(), 3);
        let release = pressed
            .before_send(&command(false, "keyUp"), &sessions)
            .unwrap()
            .unwrap();
        assert_eq!(pressed.snapshot().unwrap()["target"].len(), 3);
        pressed.confirm_release(release).unwrap();
        let held = pressed.snapshot().unwrap();
        assert_eq!(held["target"].len(), 2);
        assert!(held["target"]
            .iter()
            .any(|entry| entry.get("key") == Some(&json!("Control"))));
    }
    #[test]
    fn failed_releases_stay_held_and_old_acknowledgements_cannot_retire_a_new_press() {
        for (mouse, down, up) in [
            (false, "keyDown", "keyUp"),
            (true, "mousePressed", "mouseReleased"),
        ] {
            let pressed = PressedInputs::default();
            let sessions = HashMap::from([("session".into(), "target".into())]);
            let _ = pressed
                .before_send(&command(mouse, down), &sessions)
                .unwrap();
            let release = pressed
                .before_send(&command(mouse, up), &sessions)
                .unwrap()
                .unwrap();
            assert_eq!(pressed.snapshot().unwrap()["target"].len(), 1);
            let _ = pressed
                .before_send(&command(mouse, down), &sessions)
                .unwrap();
            pressed.confirm_release(release).unwrap();
            assert_eq!(pressed.snapshot().unwrap()["target"].len(), 1);
            let release = pressed
                .before_send(&command(mouse, up), &sessions)
                .unwrap()
                .unwrap();
            pressed.confirm_release(release).unwrap();
            assert!(pressed.snapshot().unwrap().is_empty());
        }
    }
}
