use std::io;

use crate::api::client::{self, ApiClientError};
use crate::api::schema::{
    ClientOpenUrlParams, ErrorBody, ErrorResponse, Method, Request, ResponseResult,
};

const REQUEST_ID: &str = "cli:open-url";
const USAGE: &str = "usage: herdr open-url <URL>";

#[derive(Debug, PartialEq, Eq)]
struct CommandOutput {
    stdout: Option<String>,
    stderr: Option<String>,
    exit_code: i32,
}

pub(super) fn run_open_url_command(args: &[String]) -> io::Result<i32> {
    let output = execute(args, super::send_request);
    if let Some(stdout) = output.stdout {
        println!("{stdout}");
    }
    if let Some(stderr) = output.stderr {
        eprintln!("{stderr}");
    }
    Ok(output.exit_code)
}

fn execute(
    args: &[String],
    send_request: impl FnOnce(&Request) -> io::Result<serde_json::Value>,
) -> CommandOutput {
    let [url] = args else {
        return CommandOutput {
            stdout: None,
            stderr: Some(USAGE.into()),
            exit_code: 2,
        };
    };

    if let Err(error) = crate::open_url::validate(url) {
        let response = ErrorResponse {
            id: REQUEST_ID.into(),
            error: ErrorBody {
                code: "invalid_params".into(),
                message: format!("invalid URL ({})", error.class()),
            },
        };
        return CommandOutput {
            stdout: Some(serde_json::to_string(&response).expect("error response serializes")),
            stderr: Some(format!(
                "herdr open-url rejected invalid input ({})",
                error.class()
            )),
            exit_code: 1,
        };
    }

    let request = Request {
        id: REQUEST_ID.into(),
        method: Method::ClientOpenUrl(ClientOpenUrlParams { url: url.clone() }),
    };
    let response = match send_request(&request) {
        Ok(response) => response,
        Err(error) => {
            return CommandOutput {
                stdout: None,
                stderr: Some(format!("herdr open-url request failed: {error}")),
                exit_code: 1,
            };
        }
    };
    let stdout = serde_json::to_string(&response).expect("API response serializes");

    match client::parse_response_value(response) {
        Ok(success) => match success.result {
            ResponseResult::ClientOpenUrl {
                delivered: true, ..
            } => CommandOutput {
                stdout: Some(stdout),
                stderr: None,
                exit_code: 0,
            },
            ResponseResult::ClientOpenUrl {
                delivered: false,
                reason,
            } => CommandOutput {
                stdout: Some(stdout),
                stderr: Some(format!(
                    "herdr open-url was not delivered: {}",
                    reason_name(reason)
                )),
                exit_code: 1,
            },
            _ => CommandOutput {
                stdout: Some(stdout),
                stderr: Some("herdr open-url received an unexpected response".into()),
                exit_code: 1,
            },
        },
        Err(ApiClientError::ErrorResponse(error)) => CommandOutput {
            stdout: Some(stdout),
            stderr: Some(format!(
                "herdr open-url request was rejected: {}",
                error.error.code
            )),
            exit_code: 1,
        },
        Err(_) => CommandOutput {
            stdout: Some(stdout),
            stderr: Some("herdr open-url received a malformed response".into()),
            exit_code: 1,
        },
    }
}

fn reason_name(reason: crate::api::schema::ClientOpenUrlReason) -> &'static str {
    use crate::api::schema::ClientOpenUrlReason;
    match reason {
        ClientOpenUrlReason::Forwarded => "forwarded",
        ClientOpenUrlReason::DispatchedLocally => "dispatched_locally",
        ClientOpenUrlReason::NoForegroundClient => "no_foreground_client",
        ClientOpenUrlReason::ForwardFailed => "forward_failed",
        ClientOpenUrlReason::LocalDispatchFailed => "local_dispatch_failed",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::schema::{ClientOpenUrlReason, ResponseResult, SuccessResponse};

    fn response(delivered: bool, reason: ClientOpenUrlReason) -> serde_json::Value {
        serde_json::to_value(SuccessResponse {
            id: REQUEST_ID.into(),
            result: ResponseResult::ClientOpenUrl { delivered, reason },
        })
        .unwrap()
    }

    #[test]
    fn sends_the_exact_url_and_succeeds_for_each_delivery_path() {
        let url = "HTTPS://user:secret@bücher.example:8443/a%20b?q=x#frag".to_string();
        for reason in [
            ClientOpenUrlReason::Forwarded,
            ClientOpenUrlReason::DispatchedLocally,
        ] {
            let output = execute(std::slice::from_ref(&url), |request| {
                let crate::api::schema::Method::ClientOpenUrl(params) = &request.method else {
                    panic!("expected client.open_url");
                };
                assert_eq!(params.url, url);
                Ok(response(true, reason))
            });
            assert_eq!(output.exit_code, 0);
            assert!(output.stdout.unwrap().contains("client_open_url"));
            assert_eq!(output.stderr, None);
        }
    }

    #[test]
    fn exits_nonzero_for_each_non_delivery_reason() {
        for reason in [
            ClientOpenUrlReason::NoForegroundClient,
            ClientOpenUrlReason::ForwardFailed,
            ClientOpenUrlReason::LocalDispatchFailed,
        ] {
            let output = execute(&["https://example.com".into()], |_| {
                Ok(response(false, reason))
            });
            assert_eq!(output.exit_code, 1);
            assert!(output.stdout.unwrap().contains("\"delivered\":false"));
            assert!(output.stderr.unwrap().contains(reason_name(reason)));
        }
    }

    #[test]
    fn invalid_input_is_structured_and_never_sent() {
        let sentinel = "https://user:secret@example.com/has space";
        let output = execute(&[sentinel.into()], |_| {
            panic!("invalid input must not reach the socket")
        });

        assert_eq!(output.exit_code, 1);
        let stdout = output.stdout.unwrap();
        assert!(stdout.contains("invalid_params"));
        assert!(!stdout.contains(sentinel));
        assert!(!output.stderr.unwrap().contains(sentinel));
    }

    #[test]
    fn socket_failure_has_no_success_output() {
        let output = execute(&["https://example.com".into()], |_| {
            Err(io::Error::new(io::ErrorKind::ConnectionRefused, "offline"))
        });

        assert_eq!(output.exit_code, 1);
        assert_eq!(output.stdout, None);
        assert!(output.stderr.unwrap().contains("offline"));
    }

    #[test]
    fn argument_errors_use_cli_exit_status() {
        let missing = execute(&[], |_| panic!("must not send"));
        assert_eq!(missing.exit_code, 2);

        let extra = execute(&["https://example.com".into(), "extra".into()], |_| {
            panic!("must not send")
        });
        assert_eq!(extra.exit_code, 2);
    }
}
