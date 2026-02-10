# ⚡ Lightning Server & Client on Testnet4

Rust implementation of three LNURL protocols on Bitcoin Testnet4 with Core Lightning:
- **LUD-02** — Channel Request
- **LUD-03** — Withdraw Request
- **LUD-04** — Authentication

---

## 🌐 Network Access (VPN required)

The server runs behind a **WireGuard VPN**. You must be connected to it before running any client commands.

### How to connect (teacher setup)

**Step 1 — Install WireGuard**

```bash
# Ubuntu/Debian
sudo apt install wireguard

# macOS
brew install wireguard-tools
# or download the app: https://www.wireguard.com/install/

# Windows: https://www.wireguard.com/install/
```

**Step 2 — Ask linoux to add you as a peer**

Linoux runs this on the VPN server, then sends you the resulting `.conf` file:

```bash
# Generate your keypair
sudo wg genkey | tee /tmp/teacher_privkey | wg pubkey > /tmp/teacher_pubkey

# Get the server's public key
sudo wg show wg0 public-key

# Add you as a peer (assign a free IP slot, e.g. 192.168.27.3)
sudo wg set wg0 peer $(cat /tmp/teacher_pubkey) allowed-ips 192.168.27.3/32

# Save so it persists after reboot
sudo wg-quick save wg0

# The .conf file to send you — fill in SERVER_PUBKEY from the command above:
[Interface]
PrivateKey = <contents of /tmp/teacher_privkey>
Address = 192.168.27.3/32
DNS = 1.1.1.1

[Peer]
PublicKey = <SERVER_PUBKEY>
Endpoint = 82.67.177.113:51820
AllowedIPs = 192.168.27.0/24
PersistentKeepalive = 25
```

**Step 3 — Connect (teacher side)**

```bash
# Save the .conf as wg-linoux.conf, then:
sudo wg-quick up ./wg-linoux.conf

# Verify the tunnel is up
sudo wg show
ping 192.168.27.72      # should get replies
```

**Step 4 — Confirm the server is reachable**

```bash
curl http://192.168.27.72:3000/request-channel
# Expected: {"uri":"...","callback":"...","k1":"...","tag":"channelRequest"}
```

---

## ⚙️ IP Configuration

```rust
// server/src/main.rs
const IP_ADDRESS: &str = "192.168.27.72:9735";
const CALLBACK_URL: &str = "http://192.168.27.72:3000/";

// client/src/main.rs
const CLN_RPC_PATH: &str = "/home/linoux/.lightning/testnet4/lightning-rpc";
// get_node_uri() returns:
format!("{}@{}", pubkey, "192.168.27.72:9735")
```

---

## 💰 My Node

**Testnet4 funding address:**
```
tb1qphjxmslguhwl3c2l28hxqhg24yarky2rkmsmyd
```

Get free testnet4 coins: https://coinfaucet.eu/en/btc-testnet4/

```bash
lightning-cli listfunds     # verify on-chain balance
lightning-cli getinfo       # verify node is running, shows pubkey + address
```

> Requires on-chain funds to open channels (LUD-02).
> Requires channel liquidity to pay invoices (LUD-03).

---

## 🚀 Build & Run

### Server

```bash
cd server
cargo build --release
cargo run --release
```

Server starts on `0.0.0.0:3000`. Six endpoints:

| Endpoint | Protocol | Purpose |
|---|---|---|
| `GET /request-channel` | LUD-02 | Returns channel request params |
| `GET /open-channel` | LUD-02 | Callback — opens channel to client node |
| `GET /request-withdraw` | LUD-03 | Returns withdraw params (min/max/k1) |
| `GET /withdraw` | LUD-03 | Callback — pays the submitted invoice |
| `GET /auth-challenge` | LUD-04 | Returns random 32-byte k1 challenge |
| `GET /auth-response` | LUD-04 | Verifies zbase32 signature via CLN |

### Client (once VPN is connected)

```bash
cd client
cargo build --release

# LUD-02: request a channel from the server
cargo run -- request-channel http://192.168.27.72:3000

# LUD-03: withdraw sats from the server
cargo run -- request-withdraw http://192.168.27.72:3000

# LUD-04: authenticate with the server
cargo run -- auth http://192.168.27.72:3000
```

---

## 🔧 Troubleshooting

**Can't reach the server at all:**
```bash
ping 192.168.27.72              # must work first — if not, VPN is down
sudo wg show                    # check tunnel status and last handshake
sudo wg-quick down ./wg-linoux.conf && sudo wg-quick up ./wg-linoux.conf  # reconnect
curl http://192.168.27.72:3000/request-channel  # test HTTP directly
```

**CLN RPC connection refused (server side):**
```bash
ls -la ~/.lightning/testnet4/lightning-rpc  # socket must exist
lightning-cli getinfo                        # if this fails, lightningd is down
lightningd --network=testnet4 --daemon       # restart it
```

**"not enough funds" on open-channel:**
```bash
lightning-cli listfunds         # check on-chain balance
# server needs at least 100,000 sats + fees to open a channel
```

**Invoice payment fails (LUD-03):**
```bash
lightning-cli listchannels      # verify active channels exist
lightning-cli listpeers         # check peer connections
# server needs a channel with outbound liquidity to pay invoices
```

**k1 "invalid or already used":**
- k1 values are single-use — consumed on first valid callback
- Start a fresh flow from `/request-channel`, `/request-withdraw`, or `/auth-challenge`

**lnurl-auth signature rejected:**
- Client sends the `zbase` field from `signmessage`, NOT the `signature` (DER-hex) field
- Server uses CLN `checkmessage` which expects zbase32 format — this is the key difference from the standard LNURL-auth spec
