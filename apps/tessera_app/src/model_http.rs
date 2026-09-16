//! The console's way to a model: one POST, with the answer read back whole.
//!
//! Here rather than in a library for the reason `releases.rs` is: the
//! decisions about what to send and what the answer means live in
//! `tessera_bridge::assistant` with tests, and the binary supplies the wire.
//! A 4xx from a provider is an answer — its JSON says what went wrong in
//! words the person can act on — so it is returned as a body, not an error.

use std::time::Duration;

use tessera_bridge::assistant::Transport;

/// Generous, because a model composing a long reply can take most of it;
/// finite, because a hung connection must not hold a Stop button hostage.
const PATIENCE: Duration = Duration::from_secs(180);

/// A reply larger than this is not a model's answer.
const MOST: u64 = 8 * 1024 * 1024;

pub struct Http;

impl Transport for Http {
    fn post(&self, url: &str, headers: &[(String, String)], body: &str) -> Result<String, String> {
        let agent = ureq::Agent::new_with_config(
            ureq::Agent::config_builder()
                .timeout_global(Some(PATIENCE))
                .http_status_as_error(false)
                .build(),
        );
        let mut request = agent.post(url).header("User-Agent", "Tessera-Publisher");
        for (name, value) in headers {
            request = request.header(name.as_str(), value.as_str());
        }
        let response = request
            .send(body)
            .map_err(|e| format!("could not reach {url}: {e}"))?;
        response
            .into_body()
            .with_config()
            .limit(MOST)
            .read_to_string()
            .map_err(|e| format!("could not read the model's reply: {e}"))
    }
}
