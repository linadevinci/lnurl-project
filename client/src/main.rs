use anyhow::{Context, Result, anyhow};
use cln_rpc::ClnRpc;
use secp256k1::PublicKey;
use serde::Deserialize;
use std::net::IpAddr;
use std::net::Ipv4Addr;
use std::str::FromStr;
use url::Url;

// ⚠️ UPDATE THIS to match your local CLN socket path
const CLN_RPC_PATH: &str = "/home/linoux/.lightning/testnet4/lightning-rpc";

// =============================================================================
// CLI Parsing
// =============================================================================

#[derive(Debug)]
enum Commands {
    RequestChannel { url: Url },
    RequestWithdraw { url: Url },
    Auth { url: Url },
}

fn print_usage() {
    eprintln!("Usage:");
    eprintln!("  lnurl-client request-channel <url|ip:port>");
    eprintln!("  lnurl-client request-withdraw <url|ip:port>");
    eprintln!("  lnurl-client auth <url|ip:port>");
}

fn parse_url_or_ip(input: &str) -> Result<Url> {
    // First try parsing as a full URL
    if let Ok(url) = Url::parse(input) {
        return Ok(url);
    }

    // Handle IPv6 with port: [::1]:8080
    if let Some(bracket_end) = input.find("]:") {
        if input.starts_with('[') {
            let ip_part = &input[1..bracket_end];
            let port_part = &input[bracket_end + 2..];
            if port_part.parse::<u16>().is_ok() {
                if let Ok(ip) = IpAddr::from_str(ip_part) {
                    let url_str = format!("http://[{}]:{}", ip, port_part);
                    return Url::parse(&url_str)
                        .context("Failed to convert IPv6 with port to URL");
                }
            }
        }
    }

    // Handle IPv4 with port: 192.168.1.1:8080
    if let Some(colon_pos) = input.rfind(':') {
        let ip_part = &input[..colon_pos];
        let port_part = &input[colon_pos + 1..];
        if port_part.parse::<u16>().is_ok() {
            if let Ok(ip) = IpAddr::from_str(ip_part) {
                let url_str = format!("http://{}:{}", ip, port_part);
                return Url::parse(&url_str)
                    .context("Failed to convert IP:port to URL");
            }
        }
    }

    // Plain IP with no port
    if let Ok(ip) = IpAddr::from_str(input) {
        let url_str = format!("http://{}", ip);
        return Url::parse(&url_str).context("Failed to convert IP to URL");
    }

    Err(anyhow!("Invalid URL or IP address: {}", input))
}

fn parse_args() -> Result<Commands> {
    let args: Vec<String> = std::env::args().collect();

    if args.len() < 2 {
        print_usage();
        return Err(anyhow!("No command provided"));
    }

    match args[1].as_str() {
        "request-channel" => {
            if args.len() < 3 {
                return Err(anyhow!("request-channel requires a <url> argument"));
            } else if args.len() > 3 {
                return Err(anyhow!("request-channel does not accept additional arguments"));
            }
            Ok(Commands::RequestChannel {
                url: parse_url_or_ip(&args[2])?,
            })
        }
        "request-withdraw" => {
            if args.len() < 3 {
                return Err(anyhow!("request-withdraw requires a <url> argument"));
            } else if args.len() > 3 {
                return Err(anyhow!("request-withdraw does not accept additional arguments"));
            }
            Ok(Commands::RequestWithdraw {
                url: parse_url_or_ip(&args[2])?,
            })
        }
        "auth" => {
            if args.len() < 3 {
                return Err(anyhow!("auth requires a <url> argument"));
            } else if args.len() > 3 {
                return Err(anyhow!("auth does not accept additional arguments"));
            }
            Ok(Commands::Auth {
                url: parse_url_or_ip(&args[2])?,
            })
        }
        _ => {
            print_usage();
            Err(anyhow!("Unknown command: {}", args[1]))
        }
    }
}

// =============================================================================
// CLN Helpers
// =============================================================================

/// Returns "pubkey@ip:port" URI for our own node
fn get_node_uri(ln_client: &mut ClnRpc, rt: &tokio::runtime::Runtime) -> Result<String> {
    match rt.block_on(ln_client.call(cln_rpc::Request::Getinfo(
        cln_rpc::model::requests::GetinfoRequest {},
    )))? {
        cln_rpc::model::Response::Getinfo(response) => {
            let pubkey = response.id.to_string();
            println!("Node pubkey: {}", pubkey);
            // ⚠️ UPDATE this to your node's actual listening address
            Ok(format!("{}@{}", pubkey, "192.168.27.72:49735"))
            //Ok(format!("{}@{}", pubkey, "192.168.27.72:9735"))
        }
        _ => Err(anyhow!("Unexpected response type from getinfo")),
    }
}

/// Returns just the hex pubkey of our own node
fn get_node_pubkey(ln_client: &mut ClnRpc, rt: &tokio::runtime::Runtime) -> Result<String> {
    match rt.block_on(ln_client.call(cln_rpc::Request::Getinfo(
        cln_rpc::model::requests::GetinfoRequest {},
    )))? {
        cln_rpc::model::Response::Getinfo(response) => Ok(response.id.to_string()),
        _ => Err(anyhow!("Unexpected response type from getinfo")),
    }
}

fn connect_to_node(
    ln_client: &mut ClnRpc,
    rt: &tokio::runtime::Runtime,
    node_uri: &str,
) -> Result<()> {
    let parsed = node_uri.split('@').collect::<Vec<&str>>();
    if parsed.len() != 2 {
        return Err(anyhow!("Invalid node URI: {}", node_uri));
    }
    let pubkey = PublicKey::from_str(parsed[0])?;
    let host = parsed[1];
    let parts = host.split(':').collect::<Vec<&str>>();
    let ip_addr: Ipv4Addr = parts[0].parse()?;
    let port: u16 = parts[1].parse()?;

    println!("Connecting to node {}@{}:{}...", pubkey, ip_addr, port);

    let request = cln_rpc::model::requests::ConnectRequest {
        id: pubkey.to_string(),
        host: Some(ip_addr.to_string()),
        port: Some(port),
    };

    rt.block_on(ln_client.call(cln_rpc::Request::Connect(request)))?;
    println!("Connected.");
    Ok(())
}

// =============================================================================
// request-channel (LUD-02)
// =============================================================================

#[derive(Debug, Deserialize)]
struct ChannelRequestResponse {
    uri: String,
    callback: String,
    k1: String,
}

#[derive(Debug, Deserialize)]
struct ChannelOpenResponse {
    status: String,
    reason: Option<String>,
    txid: Option<String>,
    channel_id: Option<String>,
}

fn channel_request(url: &Url) -> Result<()> {
    println!("Requesting channel info from {}...", url);

    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_io()
        .build()
        .context("Failed to create Tokio runtime")?;
    let mut ln_client = rt.block_on(cln_rpc::ClnRpc::new(CLN_RPC_PATH))?;

    // Get our pubkey (truncated to just the hex, no @host:port)
    let mut node_uri = get_node_uri(&mut ln_client, &rt)?;
    println!("Node URI: {}", node_uri);

    // Step 1: GET /request-channel
    let request_url = format!("{}/request-channel", url.as_str().trim_end_matches('/'));
    let resp: ChannelRequestResponse = ureq::get(&request_url).call()?.into_json()?;

    println!("Received channel request:");
    println!("  URI: {}", resp.uri);
    println!("  Callback: {}", resp.callback);
    println!("  k1: {}", resp.k1);

    // Step 2: Connect to the server's Lightning node
    connect_to_node(&mut ln_client, &rt, &resp.uri)?;

    // Step 3: Strip the @host:port part to get just the pubkey hex
    //         secp256k1 compressed pubkey = 33 bytes = 66 hex chars
    let _ = node_uri.split_off(secp256k1::constants::PUBLIC_KEY_SIZE * 2);

    // Step 4: Call open-channel callback
    let open_url = format!(
        "{}?remoteid={}&k1={}&private=0",
        resp.callback, node_uri, resp.k1
    );
    println!("Open URL: {}", open_url);

    let open_resp = match ureq::get(&open_url).call() {
        Ok(resp) => resp.into_json::<ChannelOpenResponse>()?,
        Err(e) => return Err(anyhow!("Failed to open channel: {}", e)),
    };

    println!("Open response: {:?}", open_resp);

    if open_resp.status == "OK" {
        println!("Channel opened successfully!");
        if let Some(txid) = open_resp.txid {
            println!("  Transaction ID: {}", txid);
        }
        if let Some(channel_id) = open_resp.channel_id {
            println!("  Channel ID: {}", channel_id);
        }
    } else {
        eprintln!(
            "Channel open failed: {}",
            open_resp.reason.unwrap_or_else(|| "unknown".to_string())
        );
    }

    Ok(())
}

// =============================================================================
// request-withdraw (LUD-03)
// =============================================================================

#[derive(Debug, Deserialize)]
#[allow(non_snake_case)]
struct WithdrawRequestResponse {
    callback: String,
    k1: String,
    tag: String,
    defaultDescription: Option<String>,
    minWithdrawable: u64, // millisatoshis
    maxWithdrawable: u64, // millisatoshis
}

#[derive(Debug, Deserialize)]
struct WithdrawCallbackResponse {
    status: String,
    reason: Option<String>,
}

fn withdraw_request(url: &Url) -> Result<()> {
    println!("Requesting withdraw info from {}...", url);

    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_io()
        .build()
        .context("Failed to create Tokio runtime")?;
    let mut ln_client = rt.block_on(cln_rpc::ClnRpc::new(CLN_RPC_PATH))?;

    // Step 1: GET /request-withdraw
    let request_url = format!("{}/request-withdraw", url.as_str().trim_end_matches('/'));
    let resp: WithdrawRequestResponse = ureq::get(&request_url).call()?.into_json()?;

    println!("Received withdraw request:");
    println!("  Callback: {}", resp.callback);
    println!("  k1: {}", resp.k1);
    println!("  Tag: {}", resp.tag);
    println!("  Min withdrawable: {} msat", resp.minWithdrawable);
    println!("  Max withdrawable: {} msat", resp.maxWithdrawable);
    if let Some(ref desc) = resp.defaultDescription {
        println!("  Description: {}", desc);
    }

    // Step 2: Pick an amount (withdraw the maximum available)
    let withdraw_amount_msat = resp.maxWithdrawable;
    println!("\nWithdrawing {} msat...", withdraw_amount_msat);

    // Step 3: Create a BOLT-11 invoice via CLN
    let label = format!(
        "lnurl-withdraw-{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    );

    let description = resp.defaultDescription
        .as_deref()
        .unwrap_or("LNURL withdraw");

    let invoice_request = cln_rpc::model::requests::InvoiceRequest {
        amount_msat: cln_rpc::primitives::AmountOrAny::Amount(
            cln_rpc::primitives::Amount::from_msat(withdraw_amount_msat),
        ),
        label: label.clone(),
        description: description.to_string(),
        expiry: Some(600),
        fallbacks: None,
        preimage: None,
        cltv: None,
        deschashonly: None,
        exposeprivatechannels: None,
    };

    let bolt11 = match rt.block_on(ln_client.call(cln_rpc::Request::Invoice(invoice_request)))? {
        cln_rpc::Response::Invoice(inv) => {
            println!("Created invoice: {}", inv.bolt11);
            inv.bolt11
        }
        _ => return Err(anyhow!("Unexpected response from invoice creation")),
    };

    // Step 4: GET /withdraw?k1=<k1>&pr=<bolt11>
    let callback_url = format!("{}?k1={}&pr={}", resp.callback, resp.k1, bolt11);
    println!("Calling withdraw callback: {}", callback_url);

    let cb_resp: WithdrawCallbackResponse = ureq::get(&callback_url).call()?.into_json()?;
    println!("Withdraw response: {:?}", cb_resp);

    if cb_resp.status == "OK" {
        println!("\nWithdraw request accepted! Waiting for incoming payment...");

        // Step 5: Block until the invoice is paid
        let wait_request = cln_rpc::model::requests::WaitinvoiceRequest { label };
        match rt.block_on(ln_client.call(cln_rpc::Request::WaitInvoice(wait_request)))? {
            cln_rpc::Response::WaitInvoice(inv) => {
                println!("Payment received!");
                println!("  Amount: {:?}", inv.amount_received_msat);
                println!("  Paid at: {:?}", inv.paid_at);
            }
            _ => println!("Unexpected response while waiting for invoice"),
        }
    } else {
        eprintln!(
            "Withdraw failed: {}",
            cb_resp.reason.unwrap_or_else(|| "unknown".to_string())
        );
    }

    Ok(())
}

// =============================================================================
// lnurl-auth (LUD-04)
// =============================================================================
//
// Flow:
//   1. GET /auth-challenge          → { k1: "<hex 32 bytes>" }
//   2. Sign k1 using CLN signmessage
//   3. GET /auth-response?k1=<k1>&signature=<zbase>&pubkey=<node_pubkey>
//
// ⚠️  The "catch": send the `zbase` field from signmessage's response,
//     NOT the `signature` (DER-hex) field. The server uses CLN checkmessage
//     which expects zbase format.

#[derive(Debug, Deserialize)]
struct AuthChallengeResponse {
    k1: String,
}

#[derive(Debug, Deserialize)]
struct AuthResponse {
    status: String,
    #[serde(default)]
    event: Option<String>,
    #[serde(default)]
    reason: Option<String>,
}

fn auth(url: &Url) -> Result<()> {
    println!("Starting LNURL-auth with {}...", url);

    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_io()
        .build()
        .context("Failed to create Tokio runtime")?;
    let mut ln_client = rt.block_on(cln_rpc::ClnRpc::new(CLN_RPC_PATH))?;

    // Step 1: Get our node pubkey
    let pubkey = get_node_pubkey(&mut ln_client, &rt)?;
    println!("Node pubkey: {}", pubkey);

    // Step 2: GET /auth-challenge
    let challenge_url = format!("{}/auth-challenge", url.as_str().trim_end_matches('/'));
    println!("Requesting auth challenge from {}...", challenge_url);
    let challenge: AuthChallengeResponse = ureq::get(&challenge_url).call()?.into_json()?;
    println!("Received k1: {}", challenge.k1);

    // Step 3: Sign k1 using CLN signmessage
    let sign_request = cln_rpc::model::requests::SignmessageRequest {
        message: challenge.k1.clone(),
    };

    let zbase = match rt.block_on(ln_client.call(cln_rpc::Request::SignMessage(sign_request)))? {
        cln_rpc::Response::SignMessage(resp) => {
            println!("Signature (hex DER): {}", resp.signature);
            println!("Recid: {}", resp.recid);
            println!("Zbase: {}", resp.zbase);
            resp.zbase // ← use zbase, not resp.signature
        }
        _ => return Err(anyhow!("Unexpected response from signmessage")),
    };

    // Step 4: GET /auth-response?k1=<k1>&signature=<zbase>&pubkey=<pubkey>
    let auth_url = format!(
        "{}/auth-response?k1={}&signature={}&pubkey={}",
        url.as_str().trim_end_matches('/'),
        challenge.k1,
        zbase,
        pubkey
    );
    println!("Calling auth endpoint: {}", auth_url);

    let auth_resp: AuthResponse = ureq::get(&auth_url).call()?.into_json()?;
    println!("Auth response: {:?}", auth_resp);

    if auth_resp.status == "OK" {
        println!("\nAuthentication successful!");
        if let Some(event) = auth_resp.event {
            println!("  Event: {}", event);
        }
    } else {
        eprintln!(
            "Authentication failed: {}",
            auth_resp.reason.unwrap_or_else(|| "unknown".to_string())
        );
    }

    Ok(())
}

// =============================================================================
// Main
// =============================================================================

fn main() {
    let command = match parse_args() {
        Ok(command) => command,
        Err(e) => {
            eprintln!("Error: {}", e);
            std::process::exit(1);
        }
    };

    let result = match command {
        Commands::RequestChannel { url } => channel_request(&url),
        Commands::RequestWithdraw { url } => withdraw_request(&url),
        Commands::Auth { url } => auth(&url),
    };

    if let Err(e) = result {
        eprintln!("Error: {}", e);
        std::process::exit(1);
    }
}
