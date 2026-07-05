# Rust Video Server (Core)

A high-performance real-time video communication server built in Rust, designed to power scalable WebRTC-based conferencing, messaging, and media coordination systems.

The server acts as the **core signaling and session orchestration layer** for real-time video applications, handling room management, participant state, media signaling, and event distribution.

---

## Overview

Rust Video Server provides the backend infrastructure required for real-time video communication systems, including:

- WebRTC signaling coordination
- Room lifecycle management
- Participant session tracking
- Real-time event broadcasting (WebSockets)
- Presence, join/leave tracking
- Optional recording/stream orchestration hooks
- Scalable concurrency via Tokio async runtime

It is designed to be **stateless where possible**, with optional persistence via SQLx-supported databases.

---

## Architecture

The system is built around a modular event-driven architecture:

- **WebSocket Gateway** – Handles client connections and real-time messaging
- **Room Manager** – Maintains active rooms and participant sessions in memory
- **Signaling Layer** – Routes SDP offers/answers and ICE candidates
- **Event Bus** – Broadcasts join/leave/chat/metadata events
- **Persistence Layer (optional)** – Stores room and session state in a database

---

## Tech Stack

- Rust (stable)
- Tokio (async runtime)
- Axum (HTTP + WebSocket server)
- SQLx (database access layer)
- Serde (serialization)
- Chrono (time management)
- Base64 + HMAC (secure token/signing utilities)
- WebSockets (real-time transport)

---

## Core Features

### Room Management

- Create rooms dynamically
- Join/leave rooms securely
- Automatic cleanup of empty rooms
- Session tracking per participant

### Real-Time Communication

- WebSocket-based signaling
- Low-latency event propagation
- Support for chat, presence, and media events

### Participant System

- Unique session IDs per connection
- Multi-device support (optional)
- Last-seen tracking
- Join approval workflows (optional extension)

### Media Coordination (WebRTC)

- Handles SDP offer/answer exchange
- ICE candidate routing
- Screen sharing state management
- Active presenter tracking

---

## API Overview

### Create Room

```http
POST /rooms
```

#### Request

```json
{
  "title": "Team Sync",
  "created_by": "user-id"
}
```

#### Response

```json
{
  "id": "room-id",
  "title": "Team Sync"
}
```

---

### Join Room (WebSocket)

```ws
wss://server/ws
```

#### Payload

```json
{
  "type": "JOIN",
  "room_id": "abc123",
  "name": "John Doe",
  "user_id": "user-id"
}
```

---

### Signaling Events

#### Offer

```json
{
  "type": "OFFER",
  "target": "participant-id",
  "sdp": "..."
}
```

#### Answer

```json
{
  "type": "ANSWER",
  "target": "participant-id",
  "sdp": "..."
}
```

#### ICE Candidate

```json
{
  "type": "ICE",
  "target": "participant-id",
  "candidate": "..."
}
```

---

### Presence Events

```json
{
  "type": "PARTICIPANT_JOINED",
  "user_id": "uuid",
  "name": "John Doe"
}
```

```json
{
  "type": "PARTICIPANT_LEFT",
  "user_id": "uuid"
}
```

---

## Internal Data Model

### Room

- `id`: UUID
- `title`: String
- `created_by`: User ID
- `sessions`: Map of active participants
- `created_at`: Timestamp

### Participant Session

- `user_id`: UUID
- `session_id`: UUID
- `name`: String
- `last_seen`: Timestamp
- `media_state`: mic/cam/screen

---

## Concurrency Model

The server leverages Rust’s async ecosystem:

- Each WebSocket connection runs in an independent Tokio task
- Shared state is managed via `Arc<RwLock<HashMap<...>>>`
- Non-blocking message dispatch ensures high throughput

Example pattern:

```rust
let rooms: Arc<RwLock<HashMap<String, Room>>> = ...
```

---

## Scalability Design

- Stateless WebSocket nodes (horizontally scalable)
- External DB for persistence (optional)
- Redis or message broker support (future extension)
- Room sharding strategy via consistent hashing (planned)

---

## Security Considerations

- HMAC-based token validation for room access
- Session binding to user identity
- Optional encrypted signaling payloads
- Input validation on all WebSocket messages
- Rate limiting support (recommended at gateway layer)

---

## Performance Characteristics

- Designed for low-latency signaling (<50ms internal routing target)
- Handles thousands of concurrent WebSocket connections per node
- Minimal allocation per event cycle
- Efficient memory cleanup for inactive rooms

---

## Example Flow

1. Client creates room via REST API
2. Client joins via WebSocket
3. Server registers participant session
4. Clients exchange SDP via signaling layer
5. Media streams are established peer-to-peer
6. Server maintains presence + coordination only

---

## Development Setup

### Prerequisites

- Rust (stable)
- Cargo
- PostgreSQL (optional)
- Redis (optional future scaling layer)

---

### Run Server

```bash
cargo run
```

---

### Run Tests

```bash
cargo test
```

---

## Environment Variables

```env
DATABASE_URL=postgres://user:password@localhost/db
RUST_LOG=info
SERVER_PORT=8080
```

---

## Future Enhancements

- SFU integration (Selective Forwarding Unit)
- Recording pipeline (FFmpeg hooks)
- AI moderation layer for meetings
- End-to-end encrypted media signaling
- Distributed room clustering
- Native mobile SDK support

---

## License

Proprietary / Internal Use (adjust as needed)
