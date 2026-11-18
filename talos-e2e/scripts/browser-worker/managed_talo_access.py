"""Cloud account fixtures and public managed-Talo authorization checks."""
from fixture import ACCOUNTS, TOKEN, USER, data, http, http_response

ASSISTANT = "/v1/computers/managed-worker"
RESTRICTED = ("free", "plus", "pro")


def configure_accounts():
    USER["type"] = "max"
    ACCOUNTS["browser-contract-bob"]["type"] = "max"
    for tier in RESTRICTED:
        # Same owner with a lower-tier credential exercises existing-resource access.
        ACCOUNTS[f"talo-{tier}"] = {**USER, "role": "user", "type": tier}
    ACCOUNTS["talo-admin"] = {**USER, "role": "admin", "type": "free"}


def exercise_access(base, instance=None, session=None):
    for tier in RESTRICTED:
        token = f"talo-{tier}"
        requests = [(ASSISTANT, None), (ASSISTANT, {})]
        instance_id = instance["instanceId"] if instance else "missing-instance"
        generation = instance["generation"] if instance else 1
        requests += [(f"{ASSISTANT}/{instance_id}/{action}", {
            "expectedGeneration": generation,
            **({"confirmDataLoss": True} if action == "delete" else {}),
        }) for action in ("start", "stop", "reset", "delete")]
        if instance:
            node = instance["nodeId"]
            requests += [
                (f"/v1/computers/{node}", None),
                (f"/v1/fs/directory?nodeId={node}&wakeIfSleeping=true", None),
                (f"/v1/workspace/info?nodeId={node}", None),
                ("/v1/desktop/open", {
                    "nodeId": node, "targetId": "managed", "viewerId": "fixture",
                    "generation": "fixture", "inputMode": "readOnly", "controllerId": None, "inputEpoch": None,
                }),
                ("/v1/projects", {"nodeId": node, "name": "Unauthorized", "directory": "/tmp"}),
                ("/v1/turns/start", {
                    "sessionId": f"unauthorized-{tier}", "nodeId": node,
                    "input": {"content": [{"type": "text", "text": "Do not wake Talo"}]},
                }),
            ]
            if session:
                requests += [
                    (f"/v1/sessions/{session}", None),
                    ("/v1/turns/start", {
                        "sessionId": session,
                        "input": {"content": [{"type": "text", "text": "Do not use existing Talo"}]},
                    }),
                ]
            page = data(http(base + "/v1/computers?limit=1", token=token))
            assert all(row["nodeId"] != node for row in page["data"]), page
        for route, body in requests:
            status, response = http_response(base + route, body, token)
            assert status == 403 and response["error"]["code"] == "permissionDenied", (tier, route, status, response)
    for token in (TOKEN, "talo-admin"):
        assert http_response(base + ASSISTANT, token=token)[0] == 200
    print("PASS Cloud Free/Plus/Pro denied across Talo routes; Admin and Max admitted", flush=True)
