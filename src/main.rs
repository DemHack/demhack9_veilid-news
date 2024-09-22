#![feature(async_closure)]

use anyhow::Error;
use async_std::sync::Mutex;
use chrono::prelude::*;
use clap::Parser;
use rand;
use rand::prelude::SliceRandom;
use reqwest;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::io::Write;
use std::sync::Arc;
use std::{fs, io};
use tokio::time;
use tracing::{error, info, warn};
use tracing_subscriber;
use veilid_core::{tools::*, CryptoKey, CryptoTyped, DHTSchema, RoutingContext, VeilidAPI};
use veilid_core::{Encodable, KeyPair};
use veilid_duplex::utils::CRYPTO_KIND;
use veilid_duplex::veilid::{AppLogic, AppMessage, VeilidDuplex, VeilidDuplexRoutes};

#[derive(Parser, Debug)]
struct Args {
    #[arg(long)]
    server: bool,
    #[arg(long)]
    data_path: String,
    #[arg(long)]
    tracker: Option<String>,
    #[arg(long)]
    request: Option<String>,
    #[arg(long, default_value_t = false)]
    verbose: bool,
    #[arg(long, default_value_t = false)]
    generate_keys: bool,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct Item {
    pub title: Option<String>,
    pub link: Option<String>,
    pub description: Option<String>,
    pub author: Option<String>,
    pub comments: Option<String>,
    pub pub_date: Option<String>,
    pub content: Option<String>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
struct Channel {
    pub title: String,
    pub link: String,
    pub description: String,
    pub language: Option<String>,
    pub pub_date: Option<String>,
    pub last_build_date: Option<String>,
    pub items: Vec<Item>,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
enum MessageType {
    NewsUpdate,
    NewsRequest,
    NewsResponse,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
struct VeilidNewsMessage {
    message_type: MessageType,
    channel_link: Option<String>,
    channel_dht: Option<CryptoTyped<CryptoKey>>,
    channel_dhts: Option<Vec<CryptoTyped<CryptoKey>>>,
}

#[derive(Clone)]
struct VeilidNewsApp {
    our_dht_key: CryptoTyped<CryptoKey>,
    routes: Arc<Mutex<VeilidDuplexRoutes>>,
    routing_context: RoutingContext,
    channel_registry: Arc<Mutex<Vec<(String, DateTime<Utc>, CryptoTyped<CryptoKey>)>>>,
    veilid_api: VeilidAPI,
}

impl VeilidNewsApp {
    pub fn new(app: VeilidDuplex) -> Self {
        let our_dht_key = app.our_dht_key;

        println!("Our DHT key: {}", our_dht_key);
        let veilid_api = app.api.clone();
        let routing_context = app.routing_context.clone();
        let routes = app.routes.clone();
        let channel_registry: Arc<Mutex<Vec<(String, DateTime<Utc>, CryptoTyped<CryptoKey>)>>> =
            Arc::new(Mutex::new(Vec::new()));

        Self {
            veilid_api,
            our_dht_key,
            routes,
            routing_context,
            channel_registry,
        }
    }
}

impl AppLogic<VeilidNewsMessage> for VeilidNewsApp {
    async fn on_message(&mut self, message: AppMessage<VeilidNewsMessage>) {
        let mut routes = self.routes.lock().await;
        let mut channel_registry = self.channel_registry.lock().await;
        let remote_dht_record = message.dht_record;

        let target = routes
            .get_route(
                remote_dht_record,
                self.veilid_api.clone(),
                self.routing_context.clone(),
            )
            .await;
        if target.is_err() {
            error!("error {}", target.unwrap_err());
            return;
        }
        let target = target.unwrap();

        if message.data.message_type == MessageType::NewsUpdate
            && message.data.channel_link.clone().is_some()
            && message.data.channel_dht.is_some()
        {
            channel_registry.push((
                message.data.channel_link.clone().unwrap(),
                Utc::now(),
                message.data.channel_dht.unwrap(),
            ));

            println!(
                "Channel posted to tracker: {}\t{}\t{}",
                message.data.channel_link.clone().unwrap(),
                message.data.channel_dht.unwrap(),
                channel_registry.len(),
            );
        }

        if message.data.message_type == MessageType::NewsRequest
            && message.data.channel_link.clone().is_some()
        {
            // find all sources that have been updated in the last 1 hour
            let channel_link = message
                .data
                .channel_link
                .clone()
                .unwrap()
                .trim()
                .to_string();
            let mut channel_dhts: Vec<CryptoTyped<CryptoKey>> = Vec::new();
            for (channel, _last_updated, dht_record) in channel_registry.iter() {
                let channel = channel.trim().to_string();
                if channel == channel_link {
                    channel_dhts.push(dht_record.clone());
                }
            }

            let data = VeilidNewsMessage {
                channel_link: message.data.channel_link.clone(),
                message_type: MessageType::NewsResponse,
                channel_dhts: Some(channel_dhts),
                channel_dht: None,
            };

            let mut app_message: AppMessage<VeilidNewsMessage> = AppMessage {
                data,
                dht_record: self.our_dht_key,
                uuid: "".to_string(),
            };

            match app_message.send(&self.routing_context, target).await {
                Err(e) => warn!("Failed to send message: {:?}", e),
                _ => {}
            };
        }

        if message.data.message_type == MessageType::NewsResponse
            && message.data.channel_dhts.is_some()
        {
            println!("Got something to veilid");

            let channel_dhts = message.data.channel_dhts.unwrap();

            // HERE USE PROOF
            // TODO: use proper proof

            // select random dht record
            let random_dht = channel_dhts.choose(&mut rand::thread_rng()).unwrap();
            println!("chose random dht: {}", random_dht);

            // request dht value
            let dht_value = self
                .routing_context
                .get_dht_value(*random_dht, 0, true)
                .await
                .unwrap()
                .unwrap()
                .data()
                .to_vec();
            // convert dht value to string
            let dht_value = String::from_utf8(dht_value).unwrap();
            println!("feed: {}", dht_value);
        }
    }
}

async fn read_feed(url: String) -> Result<rss::Channel, Error> {
    let content = reqwest::get(url).await?.bytes().await?;
    let channel = rss::Channel::read_from(&content[..])?;
    Ok(channel)
}

async fn create_dht_record_for_channel(
    rc: RoutingContext,
    channel: Channel,
) -> Result<(CryptoTyped<CryptoKey>, KeyPair), Error> {
    let schema = DHTSchema::dflt(1)?;

    let rec = rc.create_dht_record(schema, Some(CRYPTO_KIND)).await?;

    let dht_key = *rec.key();
    let owner = rec.owner();
    let secret = rec.owner_secret().unwrap();
    let keypair = KeyPair::new(*owner, *secret);

    info!("Setting DHT Key: {}", dht_key);

    // serialize dht record to json
    let json = serde_json::to_string(&channel)?;

    rc.set_dht_value(*rec.key(), 0, json.as_bytes().to_vec(), None)
        .await?;
    rc.close_dht_record(*rec.key()).await?;

    Ok((dht_key, keypair))
}

pub(crate) async fn update_dht_record_for_channel(
    rc: RoutingContext,
    channel: Channel,
    dht_key: CryptoTyped<CryptoKey>,
    dht_owner_keypair: KeyPair,
) -> Result<(), Error> {
    info!("Updating DHT Key: {} ", dht_key);
    let rec = rc.open_dht_record(dht_key, Some(dht_owner_keypair)).await?;

    let json = serde_json::to_string(&channel)?;

    rc.set_dht_value(*rec.key(), 0, json.as_bytes().to_vec(), None)
        .await?;
    rc.close_dht_record(*rec.key()).await?;

    Ok(())
}

#[tokio::main]
async fn main() -> Result<(), Error> {
    tracing_subscriber::fmt::init();

    let args = Args::parse();

    let node_keypair: KeyPair;
    let dht_key: CryptoTyped<CryptoKey>;
    let dht_keypair: KeyPair;

    if args.generate_keys {
        node_keypair = veilid_core::Crypto::generate_keypair(CRYPTO_KIND)
            .unwrap()
            .value;
        fs::create_dir_all(format!("{}/nodekeys", args.data_path.clone()))?;
        let encoded_key = node_keypair.key.encode();
        let encoded_secret = node_keypair.secret.encode();
        let mut key_file = std::fs::File::create(format!("{}/nodekeys/key", args.data_path))?;
        let mut secret_file = std::fs::File::create(format!("{}/nodekeys/secret", args.data_path))?;
        key_file.write_all(&encoded_key.as_bytes())?;
        secret_file.write_all(&encoded_secret.as_bytes())?;

        let app = VeilidDuplex::new(Some(node_keypair), None).await?;

        fs::create_dir_all(format!("{}/dhtkeys", args.data_path.clone()))?;
        let encoded_dht = app.our_dht_key.value.encode();
        let mut dht_file = std::fs::File::create(format!("{}/dhtkeys/dht", args.data_path))?;
        dht_file.write_all(encoded_dht.as_bytes())?;
        let encoded_key = app.dht_keypair.key.encode();
        let encoded_secret = app.dht_keypair.secret.encode();
        let mut key_file = std::fs::File::create(format!("{}/dhtkeys/key", args.data_path))?;
        let mut secret_file = std::fs::File::create(format!("{}/dhtkeys/secret", args.data_path))?;
        key_file.write_all(&encoded_key.as_bytes())?;
        secret_file.write_all(&encoded_secret.as_bytes())?;

        return Ok(());
    } else {
        let key = fs::read_to_string(format!("{}/nodekeys/key", args.data_path))?;
        let secret = fs::read_to_string(format!("{}/nodekeys/secret", args.data_path))?;
        let key = veilid_core::CryptoKey::try_decode(&key)?;
        let secret = veilid_core::CryptoKey::try_decode(&secret)?;

        node_keypair = KeyPair::new(key, secret);

        let key = fs::read_to_string(format!("{}/dhtkeys/key", args.data_path))?;
        let secret = fs::read_to_string(format!("{}/dhtkeys/secret", args.data_path))?;
        let key = veilid_core::CryptoKey::try_decode(&key)?;
        let secret = veilid_core::CryptoKey::try_decode(&secret)?;

        dht_keypair = KeyPair::new(key, secret);

        let dht = fs::read_to_string(format!("{}/dhtkeys/dht", args.data_path))?;
        dht_key = CryptoTyped::<CryptoKey> {
            kind: CRYPTO_KIND,
            value: veilid_core::CryptoKey::try_decode(&dht)?,
        }
    }

    let mut app = VeilidDuplex::new(Some(node_keypair), Some((dht_key, dht_keypair))).await?;

    if let Some(tracker_dht_str) = args.tracker {
        let tracker_dht = CryptoTyped::<CryptoKey>::from_str(&tracker_dht_str)?;

        if let Some(url) = args.request {
            println!("requesting feed: {}", url);

            let app_message: AppMessage<VeilidNewsMessage> = AppMessage {
                data: VeilidNewsMessage {
                    channel_link: Some(url),
                    message_type: MessageType::NewsRequest,
                    channel_dhts: None,
                    channel_dht: None,
                },
                dht_record: app.our_dht_key,
                uuid: "".to_string(),
            };

            app.send_message(app_message, tracker_dht).await?;
        } else {
            // read txt file with list of news sources
            let sources_path = format!("{}/sources", args.data_path.clone());
            let sources = fs::read_to_string(sources_path)?;
            let mut channels: HashMap<String, (CryptoTyped<CryptoKey>, KeyPair)> = HashMap::new();

            let mut update_channels = async || {
                // split line by line and check if it's valid url
                for source in sources.split("\n") {
                    let source = source.trim();
                    if source.is_empty() {
                        continue;
                    }

                    println!("Updating channel: {}", source);

                    let rss_channel = read_feed(source.to_string()).await.unwrap();
                    // convert rss channel to our channel
                    let veilid_news_channel = Channel {
                        title: rss_channel.title,
                        link: rss_channel.link,
                        description: rss_channel.description,
                        language: rss_channel.language,
                        pub_date: rss_channel.pub_date,
                        last_build_date: rss_channel.last_build_date,
                        items: rss_channel
                            .items
                            .iter()
                            .map(|item| Item {
                                title: item.title.clone(),
                                link: item.link.clone(),
                                description: item.description.clone(),
                                author: item.author.clone(),
                                comments: item.comments.clone(),
                                pub_date: item.pub_date.clone(),
                                content: item.content.clone(),
                            })
                            .collect(),
                    };

                    let channel_link = &veilid_news_channel.link.clone();
                    let channel_dht_key: CryptoTyped<CryptoKey>;
                    if channels.contains_key(channel_link) {
                        let (dht_key, dht_keypair) = channels.get(channel_link).unwrap();
                        channel_dht_key = dht_key.clone();
                        update_dht_record_for_channel(
                            app.routing_context.clone(),
                            veilid_news_channel,
                            *dht_key,
                            dht_keypair.clone(),
                        )
                        .await
                        .unwrap();
                    } else {
                        let (dht_key, dht_keypair) = create_dht_record_for_channel(
                            app.routing_context.clone(),
                            veilid_news_channel,
                        )
                        .await
                        .unwrap();
                        channel_dht_key = dht_key.clone();
                        channels.insert(channel_link.to_string(), (dht_key, dht_keypair));
                    }

                    let app_message: AppMessage<VeilidNewsMessage> = AppMessage {
                        data: VeilidNewsMessage {
                            channel_link: Some(channel_link.clone()),
                            message_type: MessageType::NewsUpdate,
                            channel_dhts: None,
                            channel_dht: Some(channel_dht_key),
                        },
                        dht_record: app.our_dht_key,
                        uuid: "".to_string(),
                    };

                    println!("Sending message");
                    app.send_message(app_message, tracker_dht).await.unwrap();
                    println!("Channel updated: {}", source);
                }
            };

            let mut interval = time::interval(Duration::from_secs(5 * 60));

            loop {
                interval.tick().await;
                update_channels().await;
            }
        }
    }

    let app_logic = VeilidNewsApp::new(app.clone());

    app.network_loop(app_logic).await?;
    app.api.shutdown().await;
    Ok(())
}
