use axum::{
    routing::get,
    http::StatusCode,
    Json, Router,
    extract::{Query, State},
};
use cln_rpc::{self, primitives::Sha256};
use cln_rpc::model::requests::FundchannelRequest;
use cln_rpc::primitives::{Amount, AmountOrAll};
use serde::{Deserialize, Serialize};
use uuid::Uuid;
use std::str::FromStr;
use std::sync::{Arc, OnceLock};
use std::collections::HashSet;
use tokio::sync::Mutex;
use rand::RngCore;

type SharedClient = Arc<Mutex<cln_rpc::ClnRpc>>;
type SharedK1Store = Arc<Mutex<HashSet<String>>>;

#[derive(Clone)]
struct AppState {
    client: SharedClient,
    k1_store: SharedK1Store,
}

const CHANNEL_REQUEST_TAG: &str = "channelRequest";
const WITHDRAW_REQUEST_TAG: &str = "withdrawRequest";
const DEFAULT_DESCRIPTION: &str = "Withdrawal from service";

// ⚠️ UPDATE THESE to match your actual machine
//const IP_ADDRESS: &str = "192.168.27.72:9735";

const IP_ADDRESS: &str = "192.168.27.72:49735";
const CALLBACK_URL: &str = "http://192.168.27.72:3000/";

static NODE_URI: OnceLock<String> = OnceLock::new();

// =============================================================================
// request-channel (LUD-02)
// =============================================================================

#[derive(Debug, Serialize)]
struct RequestChannelResponse {
    uri: &'static str,
    callback: String,
    k1: String,
    tag: &'static str,
}

async fn request_channel(
    State(state): State<AppState>,
) -> (StatusCode, Json<RequestChannelResponse>) {
    println!("Request channel received");
    let k1 = Uuid::new_v4().to_string();

    {
        let mut k1_store = state.k1_store.lock().await;
        k1_store.insert(k1.clone());
    }

    let response = RequestChannelResponse {
        uri: NODE_URI.get().expect("NODE_URI should be set at startup"),
        callback: format!("{}open-channel", CALLBACK_URL),
        k1,
        tag: CHANNEL_REQUEST_TAG,
    };

    println!("Request channel response: {:?}", response);
    (StatusCode::OK, Json(response))
}

// GET /open-channel?remoteid=<pubkey>&k1=<k1>&private=<bool>
#[derive(Debug, Deserialize)]
struct OpenChannelParams {
    remoteid: String,
    k1: String,
    #[serde(default)]
    private: Option<bool>,
}

#[derive(Serialize, Default)]
struct OpenChannelResponse {
    status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    reason: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    mindepth: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    channel_id: Option<Sha256>,
    #[serde(skip_serializing_if = "Option::is_none")]
    outnum: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tx: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    txid: Option<String>,
}

async fn open_channel(
    State(state): State<AppState>,
    Query(params): Query<OpenChannelParams>,
) -> (StatusCode, Json<OpenChannelResponse>) {
    println!("Open channel request received");
    println!("Params: {:?}", params);

    // Validate and consume k1 (single-use)
    let k1_valid = {
        let mut k1_store = state.k1_store.lock().await;
        k1_store.remove(&params.k1)
    };

    if !k1_valid {
        return (
            StatusCode::BAD_REQUEST,
            Json(OpenChannelResponse {
                status: "ERROR".to_string(),
                reason: Some("Invalid or already used k1".to_string()),
                ..Default::default()
            }),
        );
    }

    let node_id = match params.remoteid.parse() {
        Ok(id) => id,
        Err(e) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(OpenChannelResponse {
                    status: "ERROR".to_string(),
                    reason: Some(format!("Invalid node id: {}", e)),
                    ..Default::default()
                }),
            );
        }
    };

    let amount = AmountOrAll::Amount(Amount::from_sat(100_000));

    let request = FundchannelRequest {
        id: node_id,
        amount,
        announce: params.private,
        feerate: None,
        minconf: None,
        mindepth: None,
        utxos: None,
        push_msat: None,
        close_to: None,
        request_amt: None,
        compact_lease: None,
        reserve: None,
        channel_type: None,
    };

    let mut client_guard = state.client.lock().await;
    match client_guard
        .call(cln_rpc::Request::FundChannel(request))
        .await
    {
        Ok(cln_rpc::Response::FundChannel(response)) => (
            StatusCode::OK,
            Json(OpenChannelResponse {
                status: "OK".to_string(),
                reason: None,
                mindepth: Some(response.mindepth.unwrap()),
                channel_id: Some(response.channel_id),
                outnum: Some(response.outnum),
                tx: Some(response.tx),
                txid: Some(response.txid),
            }),
        ),
        Ok(_) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(OpenChannelResponse {
                status: "ERROR".to_string(),
                reason: Some("Unexpected response type".to_string()),
                ..Default::default()
            }),
        ),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(OpenChannelResponse {
                status: "ERROR".to_string(),
                reason: Some(format!("Failed to open channel: {}", e)),
                ..Default::default()
            }),
        ),
    }
}

// =============================================================================
// request-withdraw (LUD-03)
// =============================================================================

#[derive(Debug, Serialize)]
#[allow(non_snake_case)]
struct RequestWithdrawResponse {
    callback: String,
    k1: String,
    tag: &'static str,
    defaultDescription: &'static str,
    minWithdrawable: u64, // millisatoshis
    maxWithdrawable: u64, // millisatoshis
}

async fn request_withdraw(
    State(state): State<AppState>,
) -> (StatusCode, Json<RequestWithdrawResponse>) {
    println!("Request withdraw received");
    let k1 = Uuid::new_v4().to_string();

    {
        let mut k1_store = state.k1_store.lock().await;
        k1_store.insert(k1.clone());
    }

    let response = RequestWithdrawResponse {
        callback: format!("{}withdraw", CALLBACK_URL),
        k1,
        tag: WITHDRAW_REQUEST_TAG,
        defaultDescription: DEFAULT_DESCRIPTION,
        minWithdrawable: 1_000,       // 1 sat in msats
        maxWithdrawable: 1_000_000,   // 1000 sats in msats
    };

    println!("Request withdraw response: {:?}", response);
    (StatusCode::OK, Json(response))
}

// GET /withdraw?k1=<k1>&pr=<bolt11>
#[derive(Debug, Deserialize)]
struct WithdrawParams {
    k1: String,
    pr: String, // BOLT-11 invoice
}

#[derive(Serialize, Default)]
struct WithdrawResponse {
    status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    reason: Option<String>,
}

async fn withdraw(
    State(state): State<AppState>,
    Query(params): Query<WithdrawParams>,
) -> (StatusCode, Json<WithdrawResponse>) {
    println!("Withdraw request received");
    println!("  k1: {}", params.k1);
    println!("  pr: {}", params.pr);

    // Validate and consume k1
    let k1_valid = {
        let mut k1_store = state.k1_store.lock().await;
        k1_store.remove(&params.k1)
    };

    if !k1_valid {
        return (
            StatusCode::BAD_REQUEST,
            Json(WithdrawResponse {
                status: "ERROR".to_string(),
                reason: Some("Invalid or already used k1".to_string()),
            }),
        );
    }

    // Decode invoice and validate amount
    let mut client_guard = state.client.lock().await;

    let decode_request = cln_rpc::model::requests::DecodeRequest {
        string: params.pr.clone(),
    };

    let invoice_amount_msat = match client_guard
        .call(cln_rpc::Request::Decode(decode_request))
        .await
    {
        Ok(cln_rpc::Response::Decode(decoded)) => {
            match decoded.amount_msat {
                Some(amount) => {
                    let msat = amount.msat();
                    println!("  Invoice amount: {} msat", msat);
                    if msat < 1_000 {
                        return (
                            StatusCode::BAD_REQUEST,
                            Json(WithdrawResponse {
                                status: "ERROR".to_string(),
                                reason: Some(format!(
                                    "Amount {} msat below minimum 1000 msat", msat
                                )),
                            }),
                        );
                    }
                    if msat > 1_000_000 {
                        return (
                            StatusCode::BAD_REQUEST,
                            Json(WithdrawResponse {
                                status: "ERROR".to_string(),
                                reason: Some(format!(
                                    "Amount {} msat exceeds maximum 1000000 msat", msat
                                )),
                            }),
                        );
                    }
                    msat
                }
                None => {
                    return (
                        StatusCode::BAD_REQUEST,
                        Json(WithdrawResponse {
                            status: "ERROR".to_string(),
                            reason: Some("Invoice has no amount".to_string()),
                        }),
                    );
                }
            }
        }
        Ok(_) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(WithdrawResponse {
                    status: "ERROR".to_string(),
                    reason: Some("Failed to decode invoice".to_string()),
                }),
            );
        }
        Err(e) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(WithdrawResponse {
                    status: "ERROR".to_string(),
                    reason: Some(format!("Invalid invoice: {}", e)),
                }),
            );
        }
    };

    // Pay the invoice asynchronously — return OK immediately, pay in background
    // Per the LNURL spec: server "attempts to pay the invoice asynchronously"
    let bolt11 = params.pr.clone();
    let client_clone = state.client.clone();
    println!("Accepted withdraw for {} msat, paying asynchronously...", invoice_amount_msat);

    tokio::spawn(async move {
        let mut client = client_clone.lock().await;
        let pay_request = cln_rpc::model::requests::PayRequest {
            bolt11,
            amount_msat: None,
            label: None,
            riskfactor: None,
            maxfeepercent: Some(1.0),
            retry_for: Some(60),
            maxdelay: None,
            exemptfee: None,
            localinvreqid: None,
            exclude: None,
            maxfee: None,
            description: None,
            partial_msat: None,
        };

        match client.call(cln_rpc::Request::Pay(pay_request)).await {
            Ok(cln_rpc::Response::Pay(pay_resp)) => {
                println!("Withdraw payment successful!");
                println!("  Payment preimage: {:?}", pay_resp.payment_preimage);
                println!("  Amount sent: {:?}", pay_resp.amount_sent_msat);
            }
            Ok(_) => eprintln!("Unexpected response type from pay"),
            Err(e) => eprintln!("Withdraw payment failed: {}", e),
        }
    });

    (
        StatusCode::OK,
        Json(WithdrawResponse {
            status: "OK".to_string(),
            reason: None,
        }),
    )
}

// =============================================================================
// lnurl-auth (LUD-04)
// =============================================================================
//
// Flow:
//   1. GET /auth-challenge  → { k1: "<hex 32 random bytes>" }
//   2. Client signs k1 with their node key via CLN signmessage
//   3. GET /auth-response?k1=<k1>&signature=<zbase>&pubkey=<node_pubkey>
//   4. Server verifies via CLN checkmessage
//
// ⚠️  The "catch": CLN checkmessage expects zbase-encoded signatures,
//     NOT DER-hex as the standard LNURL-auth spec describes.
//     signmessage returns { signature, recid, zbase } — use the `zbase` field.

#[derive(Debug, Serialize)]
struct AuthChallengeResponse {
    k1: String,
}

async fn auth_challenge(
    State(state): State<AppState>,
) -> (StatusCode, Json<AuthChallengeResponse>) {
    let mut random_bytes = [0u8; 32];
    rand::thread_rng().fill_bytes(&mut random_bytes);
    let k1 = random_bytes
        .iter()
        .map(|b| format!("{:02x}", b))
        .collect::<String>();

    println!("Auth challenge issued: {}", k1);

    {
        let mut k1_store = state.k1_store.lock().await;
        k1_store.insert(k1.clone());
    }

    (StatusCode::OK, Json(AuthChallengeResponse { k1 }))
}

#[derive(Debug, Deserialize)]
struct AuthResponseParams {
    k1: String,
    signature: String, // zbase-encoded (NOT DER-hex)
    pubkey: String,    // hex-encoded compressed node pubkey
}

#[derive(Debug, Serialize)]
struct AuthResult {
    status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    event: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    reason: Option<String>,
}

async fn auth_response(
    State(state): State<AppState>,
    Query(params): Query<AuthResponseParams>,
) -> (StatusCode, Json<AuthResult>) {
    println!("Auth response received:");
    println!("  k1: {}", params.k1);
    println!("  signature (zbase): {}", params.signature);
    println!("  pubkey: {}", params.pubkey);

    // Validate and consume k1
    let k1_valid = {
        let mut k1_store = state.k1_store.lock().await;
        k1_store.remove(&params.k1)
    };

    if !k1_valid {
        return (
            StatusCode::BAD_REQUEST,
            Json(AuthResult {
                status: "ERROR".to_string(),
                event: None,
                reason: Some("Invalid or expired k1".to_string()),
            }),
        );
    }

    // Validate pubkey format
    let pubkey = match cln_rpc::primitives::PublicKey::from_str(&params.pubkey) {
        Ok(pk) => pk,
        Err(e) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(AuthResult {
                    status: "ERROR".to_string(),
                    event: None,
                    reason: Some(format!("Invalid pubkey: {}", e)),
                }),
            );
        }
    };

    // Verify signature via CLN checkmessage
    let check_request = cln_rpc::model::requests::CheckmessageRequest {
        message: params.k1.clone(),
        zbase: params.signature.clone(),
        pubkey: Some(pubkey),
    };

    let mut client_guard = state.client.lock().await;
    match client_guard
        .call(cln_rpc::Request::CheckMessage(check_request))
        .await
    {
        Ok(cln_rpc::Response::CheckMessage(check_resp)) => {
            if check_resp.verified {
                println!("Auth SUCCESS for pubkey {}", params.pubkey);
                (
                    StatusCode::OK,
                    Json(AuthResult {
                        status: "OK".to_string(),
                        event: Some("LOGGEDIN".to_string()),
                        reason: None,
                    }),
                )
            } else {
                println!("Auth FAILED: signature not verified");
                (
                    StatusCode::UNAUTHORIZED,
                    Json(AuthResult {
                        status: "ERROR".to_string(),
                        event: None,
                        reason: Some("Signature verification failed".to_string()),
                    }),
                )
            }
        }
        Ok(_) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(AuthResult {
                status: "ERROR".to_string(),
                event: None,
                reason: Some("Unexpected response from checkmessage".to_string()),
            }),
        ),
        Err(e) => {
            eprintln!("checkmessage error: {}", e);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(AuthResult {
                    status: "ERROR".to_string(),
                    event: None,
                    reason: Some(format!("Verification error: {}", e)),
                }),
            )
        }
    }
}

// =============================================================================
// Main
// =============================================================================

#[tokio::main]
async fn main() {
    let home = std::env::var("HOME").expect("HOME env var not set");
    let rpc_path = format!("{home}/.lightning/testnet4/lightning-rpc");

    let client = match cln_rpc::ClnRpc::new(&rpc_path).await {
        Ok(c) => c,
        Err(e) => {
            eprintln!("Failed to connect to CLN RPC at {}: {}", rpc_path, e);
            std::process::exit(1);
        }
    };

    let shared_client = Arc::new(Mutex::new(client));
    let k1_store: SharedK1Store = Arc::new(Mutex::new(HashSet::new()));

    let app_state = AppState {
        client: shared_client.clone(),
        k1_store: k1_store.clone(),
    };

    // Fetch node pubkey at startup and cache in NODE_URI
    let node_info = shared_client
        .lock()
        .await
        .call(cln_rpc::Request::Getinfo(
            cln_rpc::model::requests::GetinfoRequest {},
        ))
        .await;

    match node_info {
        Ok(cln_rpc::model::Response::Getinfo(response)) => {
            let pubkey = response.id.to_string();
            NODE_URI
                .set(format!("{}@{}", pubkey, IP_ADDRESS))
                .expect("Failed to set NODE_URI");
            println!("Node initialized: {}", NODE_URI.get().unwrap());
        }
        Err(e) => {
            eprintln!("Failed to get node info: {}", e);
            std::process::exit(1);
        }
        _ => {
            eprintln!("Unexpected response type from getinfo");
            std::process::exit(1);
        }
    }

    let app = Router::new()
        // LUD-02: Channel Request
        .route("/request-channel", get(request_channel))
        .route("/open-channel", get(open_channel))
        // LUD-03: Withdraw Request
        .route("/request-withdraw", get(request_withdraw))
        .route("/withdraw", get(withdraw))
        // LUD-04: Auth
        .route("/auth-challenge", get(auth_challenge))
        .route("/auth-response", get(auth_response))
        .with_state(app_state);

    println!("LNURL server listening on 0.0.0.0:3000");
    println!("Endpoints:");
    println!("  GET /request-channel   - LUD-02 channel request");
    println!("  GET /open-channel      - LUD-02 channel open callback");
    println!("  GET /request-withdraw  - LUD-03 withdraw request");
    println!("  GET /withdraw          - LUD-03 withdraw callback");
    println!("  GET /auth-challenge    - LUD-04 auth challenge");
    println!("  GET /auth-response     - LUD-04 auth verify");

    let listener = tokio::net::TcpListener::bind("0.0.0.0:3000").await.unwrap();
    axum::serve(listener, app).await.unwrap();
}
