# lightning Server & Client on Testnet4

Rust implementation of three LNURL protocols on Bitcoin Testnet4 with Core Lightning:
-Channel Request, Withdraw Request & Authentication 

---

## ⚠️ ip config

The files are already pre-filled with my addresses:

```rust
// server/src/main.rs
const IP_ADDRESS: &str = "192.168.27.67:9735";
const CALLBACK_URL: &str = "http://192.168.27.67:3000/";

// client/src/main.rs
const CLN_RPC_PATH: &str = "/home/linoux/.lightning/testnet4/lightning-rpc";
// get_node_uri() returns:
format!("{}@{}", pubkey, "127.0.0.1:49735")
```

---
### MY NODE

```bash
lightning-cli newaddr          
# → go to a testnet4 faucet and send coins there
# e.g. https://coinfaucet.eu/en/btc-testnet4/

lightning-cli listfunds        # confirm balance shows up
```
### My node
**tb1qphjxmslguhwl3c2l28hxqhg24yarky2rkmsmyd**

need on-chain funds to open channels (LUD-02).  
need channel liquidity to pay invoices (LUD-03).

---

## 🚀 Build & Run

### Server

```bash
cd server
cargo build --release
cargo run --release
```

Server starts on `0.0.0.0:3000`. Six endpoints:

| Endpoint | Protocol | Method |
|---|---|---|
| `GET /request-channel` | LUD-02 | Returns channel request params |
| `GET /open-channel` | LUD-02 | Callback — opens channel to client |
| `GET /request-withdraw` | LUD-03 | Returns withdraw params |
| `GET /withdraw` | LUD-03 | Callback — pays the invoice |
| `GET /auth-challenge` | LUD-04 | Returns random 32-byte k1 challenge |
| `GET /auth-response` | LUD-04 | Verifies zbase signature via CLN |

### Client

```bash
cd client
cargo build --release

# LUD-02: request a channel from the server
cargo run -- request-channel http://SERVER_IP:3000

# LUD-03: withdraw sats from the server
cargo run -- request-withdraw http://SERVER_IP:3000

# LUD-04: authenticate with the server
cargo run -- auth http://SERVER_IP:3000
```

---

## 🔧 Troubleshooting

**CLN RPC connection refused:**
```bash
ls -la ~/.lightning/testnet4/lightning-rpc   # socket must exist
lightning-cli getinfo                         # verify daemon is running
```

**"not enough funds" on fundchannel:**
```bash
lightning-cli listfunds    # check on-chain balance
# need at least 100k sats + fees
```

**Invoice payment fails:**
- Server needs a channel with outbound liquidity
- Use `lightning-cli listchannels` to verify channels are active

**k1 "invalid or already used":**
- k1 values are single-use (consumed on first valid callback)
- Start a fresh flow from `/request-channel` or `/request-withdraw`
