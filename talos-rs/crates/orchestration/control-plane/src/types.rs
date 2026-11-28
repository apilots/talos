/// Produced only by the node authentication boundary. Never log hashes.
pub enum NodePrincipal {
    Registration {
        node_id: String,
        credential_hash: [u8; 32],
    },
    Lease {
        node_id: String,
        token_hash: [u8; 32],
    },
}
