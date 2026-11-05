//! The advertised schema and typed Tool decoder share the same closed operations.
use super::*;

fn valid_requests() -> Vec<Value> {
    vec![
        json!({"target":"goal","action":{"type":"create","objective":"Fix tests","tokenBudget":null}}),
        json!({"target":"goal","action":{"type":"read"}}),
        json!({"target":"goal","action":{"type":"edit","goalId":"g","expectedRevision":2,"objective":"Verify regression"}}),
        json!({"target":"goal","action":{"type":"pause","goalId":"g"}}),
        json!({"target":"goal","action":{"type":"resume","goalId":"g"}}),
        json!({"target":"goal","action":{"type":"clear","goalId":"g"}}),
        json!({"target":"goal","action":{"type":"complete","goalId":"g","objectiveRevision":2,"evidence":"Checks pass"}}),
        json!({"target":"loop","action":{"type":"create","prompt":"Check CI","schedule":{"type":"fixedInterval","minutes":5}}}),
        json!({"target":"loop","action":{"type":"create","prompt":"Follow deployment","schedule":{"type":"modelDecided"}}}),
        json!({"target":"loop","action":{"type":"list"}}),
        json!({"target":"loop","action":{"type":"edit","loopId":"l","expectedRevision":3,"prompt":"Check CI","schedule":{"type":"fixedInterval","minutes":1}}}),
        json!({"target":"loop","action":{"type":"stop","loopId":"l"}}),
        json!({"target":"loop","action":{"type":"clear","loopId":"l"}}),
        json!({"target":"loop","action":{"type":"decide","loopId":"l","configRevision":3,"decision":{"type":"continue","delayMinutes":1,"reason":"Pending deployment"}}}),
        json!({"target":"loop","action":{"type":"decide","loopId":"l","configRevision":3,"decision":{"type":"stop","reason":"Deployment complete"}}}),
    ]
}

#[tokio::test]
async fn every_valid_operation_forwards_the_same_typed_request() {
    let control = control(AutomationEffect::Applied);
    let inputs = valid_requests();
    for input in &inputs {
        AutomationTool.validate_input(input).unwrap();
        AutomationTool
            .execute_payload(input.clone(), context(control.clone()))
            .await
            .unwrap();
    }
    let requests = control.requests.lock().unwrap();
    assert_eq!(requests.len(), inputs.len());
    for (request, input) in requests.iter().zip(inputs) {
        assert_eq!(serde_json::to_value(&request.action).unwrap(), input);
        assert_eq!(request.caller.session_id, "session");
        assert_eq!(request.call_id, "runtime-call");
    }
}

#[tokio::test]
async fn malformed_or_unknown_nested_arguments_never_reach_control() {
    let control = control(AutomationEffect::Applied);
    let mut invalid = vec![
        json!("{\"target\":\"goal\",\"action\":{\"type\":\"read\"}}"),
        json!({"target":"goal","action":"{\"type\":\"read\"}"}),
        json!({"target":"goal","action":[]}),
        json!({"target":"goal","action":{"type":"stop","loopId":"l"}}),
        json!({"target":"loop","action":{"type":"pause","goalId":"g"}}),
        json!({"target":"loop","action":{"type":"create","prompt":"Check","schedule":"modelDecided"}}),
        json!({"target":"loop","action":{"type":"create","prompt":"Check","schedule":{"type":"modelDecided","minutes":1}}}),
        json!({"target":"loop","action":{"type":"create","prompt":"Check","schedule":{"type":"fixedInterval","minutes":0}}}),
        json!({"target":"loop","action":{"type":"create","prompt":"Check","schedule":{"type":"fixedInterval","minutes":"1"}}}),
        json!({"target":"loop","action":{"type":"decide","loopId":"l","configRevision":1,"decision":{"type":"continue","delayMinutes":0,"reason":"Now"}}}),
        json!({"target":"loop","action":{"type":"decide","loopId":"l","configRevision":1,"decision":"stop"}}),
        json!({"target":"loop","action":{"type":"decide","loopId":"l","configRevision":1,"decision":{"type":"stop","reason":"Done","delayMinutes":1}}}),
        json!({"target":"goal","action":{"type":"edit","goalId":"g","objective":"Missing revision"}}),
    ];
    // Exercise each operation's closed shape, including otherwise empty Read/List.
    for mut input in valid_requests() {
        input["action"]["unexpected"] = json!(true);
        invalid.push(input);
    }
    let mut spoofed = json!({"target":"goal","action":{"type":"read"}});
    spoofed["caller"] = json!({"sessionId":"foreign"});
    invalid.push(spoofed);
    for input in invalid {
        assert!(AutomationTool.validate_input(&input).is_err(), "{input}");
        let error = AutomationTool
            .execute_payload(input.clone(), context(control.clone()))
            .await
            .unwrap_err();
        assert!(error.is_code(ErrorCode::InvalidRequest), "{input}");
    }
    assert!(control.requests.lock().unwrap().is_empty());
}

#[test]
fn direct_object_schema_reuses_closed_tagged_actions_and_minimums() {
    let schema = AutomationTool.parameters_schema().unwrap();
    let expected = schema["oneOf"]
        .as_array()
        .unwrap()
        .iter()
        .map(|variant| variant["properties"]["action"].clone())
        .collect::<Vec<_>>();
    assert_eq!(schema["properties"]["action"]["type"], "object");
    assert_eq!(schema["properties"]["action"]["anyOf"], json!(expected));
    for name in [
        "GoalAction",
        "LoopAction",
        "LoopWakeDecision",
        "LoopSchedule",
    ] {
        let definition = &schema["definitions"][name];
        for variant in definition["oneOf"].as_array().unwrap() {
            assert_eq!(variant["additionalProperties"], false, "{name}: {variant}");
        }
    }
    for (name, field) in [
        ("LoopSchedule", "minutes"),
        ("LoopWakeDecision", "delayMinutes"),
    ] {
        let variant = schema["definitions"][name]["oneOf"]
            .as_array()
            .unwrap()
            .iter()
            .find(|variant| variant["properties"].get(field).is_some())
            .unwrap();
        assert_eq!(variant["properties"][field]["minimum"].as_f64(), Some(1.0));
    }
    assert_eq!(schema["additionalProperties"], false);
    assert_eq!(schema["required"], json!(["target", "action"]));
}

#[test]
fn skill_argument_examples_match_the_real_tool_decoder() {
    for (name, document) in [
        (
            "goal",
            include_str!("../../../skills/builtins/goal/SKILL.md"),
        ),
        (
            "loop",
            include_str!("../../../skills/builtins/loop/SKILL.md"),
        ),
    ] {
        let mut examples = 0;
        for segment in document.split('`') {
            let segment = segment.trim();
            let candidate = segment.strip_prefix("json\n").unwrap_or(segment).trim();
            if !candidate.starts_with("{\"target\"") {
                continue;
            }
            let input: Value =
                serde_json::from_str(candidate).expect("Skill argument example must be JSON");
            AutomationTool
                .validate_input(&input)
                .unwrap_or_else(|error| panic!("Invalid {name} example: {error}"));
            examples += 1;
        }
        assert!(examples > 0, "{name} must provide argument examples");
    }
}
