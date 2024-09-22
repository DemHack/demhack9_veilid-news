# Veilid-News

Veilid-News is a peer-to-peer, anonymous RSS news tracker, created for DemHack 9.

## Motivation

Centralized news sources can be subject to censorship or alteration based on geographic or political factors. Veilid-News decentralizes news aggregation, providing a solution to combat censorship by distributing information across a peer-to-peer network.

## How it Works

Veilid-News leverages the [Veilid](https://gitlab.com/veilid/veilid), which allows developers to create decentralized, privacy-focused applications. Veilid provides a distributed hash table (DHT) and encrypted, onion-like routing for secure peer-to-peer communication.

Veilid-News operates with three node types:

1. **Tracker**: Functions like a torrent tracker, directing clients to DHT records of relevant news sources.
2. **Source**: In this demo, a node that rebroadcasts RSS feeds to the DHT and registers sources with the tracker.
3. **Reader**: Connects to the tracker to retrieve a source’s DHT key.

All nodes interact with the same DHT. Once a node accesses a DHT record, it participates in the network and forwards the record to other clients when needed. Veilid supports various types of connections, including metered ones, and can be run on PCs, mobile devices, or the web via WebAssembly.

## Use Cases

1. **Decentralized News**: Eliminate dependence on centralized platforms prone to censorship or take-down scenarios.
2. **FOSS Adoption**: Move away from centralized messaging channels that track users while consuming news.

## How to Use

Clone the repository and follow these steps:

### Tracker Node

Generate keys:

```bash
cargo run -- --verbose --data-path ~/veilid-news-node-tracker --generate-keys
```

Start the tracker:

```bash
cargo run -- --verbose --data-path ~/veilid-news-node-tracker --server
```

Get the node's DHT record:

```
Our DHT key: VLD0:pjO2UsWxLliMPAOkJFom0HOSM_YahtCKOCqz7AZ8NaA
```

### Source Node

Generate keys:

```bash
cargo run -- --verbose --data-path ~/veilid-news-node-source --generate-keys
```

Add an RSS feed:

```bash
touch ~/veilid-news-node-source/sources
echo 'http://lenta.ru/rss/last24' > ~/veilid-news-node-source/sources
```

Start the source node

```bash
cargo run -- --verbose --data-path ~/veilid-news-node-source --tracker VLD0:pjO2UsWxLliMPAOkJFom0HOSM_YahtCKOCqz7AZ8NaA
```

### Client Node

Generate keys:

```bash
cargo run -- --verbose --data-path ~/veilid-news-node-client --generate-keys
```

Request a feed

```bash
cargo run -- --verbose --data-path ~/veilid-news-node-client --tracker VLD0:pjO2UsWxLliMPAOkJFom0HOSM_YahtCKOCqz7AZ8NaA --request http://lenta.ru
```

## Additional Information

This project is a work in progress (WIP). It may require several attempts to get the system running smoothly.