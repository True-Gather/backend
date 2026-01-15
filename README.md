# TrueGather Backend

Backend sécurisé pour la plateforme de vidéoconférence TrueGather, utilisant Rust et webrtc-rs.

## ✨ Fonctionnalités

- **REST API** - Création et gestion des salles de réunion
- **WebSocket Signaling** - Échange SDP/ICE pour établir les connexions WebRTC
- **JWT Authentication** - Tokens sécurisés avec expiration
- **Redis Integration** - Persistance des salles et sessions
- **Media Gateway (SFU)** - Relais média utilisant webrtc-rs
- **STUN/TURN Support** - Configuration des serveurs ICE

## 🏗️ Architecture

```
┌─────────────────────────────────────────────────────────────┐
│  Backend Rust (Tokio + Axum)                                │
│  ├── REST API (rooms, join, leave, health)                  │
│  ├── WebSocket Signaling (SDP, ICE, events)                 │
│  ├── Media Gateway (webrtc-rs SFU)                          │
│  ├── Auth Service (JWT)                                     │
│  └── Redis Repository (state, rooms, sessions)              │
└─────────────────────────────────────────────────────────────┘
         ↕                     ↕
    ┌────────────┐       ┌──────────────┐
    │   Redis    │       │ STUN/TURN    │
    └────────────┘       └──────────────┘
```

## 🚀 Démarrage Rapide

### Prérequis

- Rust 1.70+
- Redis 6.0+
- Docker (optionnel)

### Configuration

1. Copier le fichier d'environnement:
```bash
cp .env.example .env
```

2. Éditer `.env` avec vos valeurs:
```env
JWT_SECRET=votre-secret-super-securise
REDIS_URL=redis://localhost:6379
```

### Lancer avec Docker

```bash
# À la racine du projet
docker-compose up -d redis
cargo run
```

### Lancer en local

```bash
# Installer Redis localement
brew install redis
brew services start redis

# Lancer le backend
cargo run
```

## 📚 API Endpoints

### REST API

| Méthode | Endpoint | Description |
|---------|----------|-------------|
| `POST` | `/api/v1/rooms` | Créer une nouvelle salle |
| `GET` | `/api/v1/rooms/:id` | Récupérer les infos d'une salle |
| `POST` | `/api/v1/rooms/:id/join` | Rejoindre une salle |
| `POST` | `/api/v1/rooms/:id/leave` | Quitter une salle |
| `GET` | `/health` | Health check |

### Créer une Salle

```bash
curl -X POST http://localhost:8080/api/v1/rooms \
  -H "Content-Type: application/json" \
  -d '{"name": "Ma Réunion", "max_publishers": 10}'
```

### Rejoindre une Salle

```bash
curl -X POST http://localhost:8080/api/v1/rooms/{room_id}/join \
  -H "Content-Type: application/json" \
  -d '{"display": "Alice"}'
```

## 🔌 WebSocket Signaling

Connectez-vous au WebSocket:
```
ws://localhost:8080/ws?room_id={room_id}&token={jwt_token}
```

### Messages Client → Serveur

| Type | Description |
|------|-------------|
| `join_room` | Rejoindre la salle |
| `publish_offer` | Envoyer SDP offer pour publier |
| `trickle_ice` | Envoyer ICE candidate |
| `subscribe` | S'abonner à des flux |
| `subscribe_answer` | Répondre avec SDP answer |
| `leave` | Quitter la salle |

### Messages Serveur → Client

| Type | Description |
|------|-------------|
| `joined` | Confirmation de jonction |
| `publisher_joined` | Nouveau publisher dans la salle |
| `publisher_left` | Publisher parti |
| `publish_answer` | Réponse SDP pour publication |
| `subscribe_offer` | Offer SDP pour subscription |
| `error` | Message d'erreur |

### Exemple de Session

```javascript
// 1. Connexion WebSocket
const ws = new WebSocket('ws://localhost:8080/ws?room_id=xxx&token=yyy');

// 2. Rejoindre la salle
ws.send(JSON.stringify({
  type: 'join_room',
  request_id: '1',
  payload: { room_id: 'xxx', display: 'Alice' }
}));

// 3. Publier après getUserMedia()
ws.send(JSON.stringify({
  type: 'publish_offer',
  request_id: '2',
  payload: { sdp: offer.sdp, kind: 'video' }
}));
```

## 📦 Structure du Projet

```
backend/
├── Cargo.toml           # Dépendances
├── .env.example         # Template variables environnement
├── Dockerfile           # Build container
├── src/
│   ├── main.rs          # Point d'entrée
│   ├── lib.rs           # Module library
│   ├── config.rs        # Configuration
│   ├── error.rs         # Gestion d'erreurs
│   ├── state.rs         # État application
│   ├── api/             # REST endpoints
│   │   ├── mod.rs
│   │   ├── rooms.rs
│   │   └── health.rs
│   ├── auth/            # JWT service
│   │   └── mod.rs
│   ├── redis/           # Repository Redis
│   │   ├── mod.rs
│   │   └── room_repository.rs
│   ├── ws/              # WebSocket signaling
│   │   ├── mod.rs
│   │   ├── handler.rs
│   │   ├── messages.rs
│   │   └── session.rs
│   ├── media/           # Media Gateway
│   │   ├── mod.rs
│   │   ├── gateway.rs
│   │   └── track_forwarder.rs
│   └── models/          # Types de données
│       ├── mod.rs
│       ├── room.rs
│       └── user.rs
└── tests/               # Tests
```

## 🧪 Tests

```bash
# Tests unitaires
cargo test

# Avec logs
RUST_LOG=debug cargo test -- --nocapture
```

## 🔒 Sécurité

- **JWT Tokens** - Expiration courte (15 min par défaut)
- **DTLS-SRTP** - Chiffrement des flux média WebRTC
- **Validation stricte** - Toutes les entrées sont validées
- **Pas de logs sensibles** - SDP et données personnelles exclus

## 📝 Variables d'Environnement

| Variable | Description | Défaut |
|----------|-------------|--------|
| `SERVER_HOST` | Adresse d'écoute | `0.0.0.0` |
| `SERVER_PORT` | Port d'écoute | `8080` |
| `REDIS_URL` | URL Redis | `redis://localhost:6379` |
| `JWT_SECRET` | Secret JWT | **Requis** |
| `JWT_EXPIRY_SECONDS` | Durée token | `900` (15 min) |
| `ROOM_TTL_SECONDS` | TTL des salles | `7200` (2h) |
| `STUN_SERVER` | Serveur STUN | `stun:stun.l.google.com:19302` |
| `TURN_SERVER` | Serveur TURN | Optionnel |
| `RUST_LOG` | Niveau de log | `info` |

## 🛠️ Développement

```bash
# Hot reload avec cargo-watch
cargo install cargo-watch
cargo watch -x run

# Format du code
cargo fmt

# Linting
cargo clippy
```


Utilise la bonne commande : docker compose (pas docker-compose)

Depuis la racine de TrueGather (où est ton compose) :

cd ~/TrueGather
docker compose up -d redis


(Si tu lances depuis backend/ :)

docker compose -f ../docker-compose.yml up -d redis
## 📄 Licence

MIT License - voir [LICENSE](LICENSE)
