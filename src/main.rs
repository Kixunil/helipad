//Modules ----------------------------------------------------------------------------------------------------
//------------------------------------------------------------------------------------------------------------
use hyper::{
    body::to_bytes,
    service::{make_service_fn, service_fn},
    Body, Request, Server,
};
use route_recognizer::Params;
use router::Router;
use std::sync::Arc;
use hyper::server::conn::AddrStream;
use std::fs;
use std::env;
use drop_root::set_user_group;
use lnd;
use serde::{Deserialize, Deserializer};
use serde_json::Value;
// use hyper::http::Request;

#[macro_use]
extern crate configure_me;



//Globals ----------------------------------------------------------------------------------------------------
//------------------------------------------------------------------------------------------------------------
mod handler;
mod router;

type Response = hyper::Response<hyper::Body>;
type Error = Box<dyn std::error::Error + Send + Sync + 'static>;

const HELIPAD_CONFIG_FILE: &str = "./helipad.conf";
const HELIPAD_DATABASE_DIR: &str = "database.db";
const HELIPAD_STANDARD_PORT: &str = "2112";
const LND_STANDARD_GRPC_URL: &str = "https://127.0.0.1:10009";
const LND_STANDARD_MACAROON_LOCATION: &str = "/lnd/data/chain/bitcoin/mainnet/admin.macaroon";
const LND_STANDARD_TLSCERT_LOCATION: &str = "/lnd/tls.cert";

//Structs ----------------------------------------------------------------------------------------------------
//------------------------------------------------------------------------------------------------------------
#[derive(Clone, Debug)]
pub struct AppState {
    pub state_thing: String,
    pub remote_ip: String,
}

#[derive(Clone, Debug)]
pub struct HelipadConfig {
    pub database_file_path: String,
    pub listen_port: String,
    pub macaroon_path: String,
    pub cert_path: String,
}

#[derive(Debug)]
pub struct Context {
    pub state: AppState,
    pub req: Request<Body>,
    pub path: String,
    pub params: Params,
    pub database_file_path: String,
    body_bytes: Option<hyper::body::Bytes>,
}

#[allow(dead_code)]
#[derive(Deserialize, Debug)]
#[allow(non_snake_case)]
struct RawBoost {
    #[serde(default="d_action")]
    action: Option<String>,
    #[serde(default="d_blank")]
    app_name: Option<String>,
    #[serde(default="d_blank")]
    app_version: Option<String>,
    #[serde(default="d_blank")]
    boost_link: Option<String>,
    #[serde(default="d_blank")]
    message: Option<String>,
    #[serde(default="d_blank")]
    name: Option<String>,
    #[serde(default="d_blank")]
    pubkey: Option<String>,
    #[serde(default="d_blank")]
    sender_key: Option<String>,
    #[serde(default="d_blank")]
    sender_name: Option<String>,
    #[serde(default="d_blank")]
    sender_id: Option<String>,
    #[serde(default="d_blank")]
    sig_fields: Option<String>,
    #[serde(default="d_blank")]
    signature: Option<String>,
    #[serde(default="d_blank")]
    speed: Option<String>,
    #[serde(default="d_blank")]
    uuid: Option<String>,
    #[serde(default="d_blank")]
    podcast: Option<String>,
    #[serde(default="d_zero", deserialize_with="de_optional_string_or_number")]
    feedID: Option<u64>,
    #[serde(default="d_blank")]
    guid: Option<String>,
    #[serde(default="d_blank")]
    url: Option<String>,
    #[serde(default="d_blank")]
    episode: Option<String>,
    #[serde(default="d_zero", deserialize_with="de_optional_string_or_number")]
    itemID: Option<u64>,
    #[serde(default="d_blank")]
    episode_guid: Option<String>,
    #[serde(default="d_blank")]
    time: Option<String>,
    #[serde(default="d_zero", deserialize_with="de_optional_string_or_number")]
    ts: Option<u64>,
    #[serde(default="d_zero", deserialize_with="de_optional_string_or_number")]
    value_msat: Option<u64>,
    #[serde(default="d_zero", deserialize_with="de_optional_string_or_number")]
    value_msat_total: Option<u64>,
}



//Traits------------------------------------------------------------------------------------------------------
//------------------------------------------------------------------------------------------------------------
fn d_action() -> Option<String> {
    Some("stream".to_string())
}
fn d_blank() -> Option<String> {
    None
}
fn d_zero() -> Option<u64> {
    None
}

fn de_optional_string_or_number<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Option<u64>, D::Error> {
    Ok(match Value::deserialize(deserializer)? {
        Value::String(s) => {
            if s.is_empty() {
                return Ok(None)
            }
            Some(s.parse().unwrap())
        },
        Value::Number(num) => Some(num.as_u64().unwrap()),
        _ => Some(0)
    })
}

//Configure_me
include_config!();



//Main -------------------------------------------------------------------------------------------------------
//------------------------------------------------------------------------------------------------------------
#[tokio::main]
async fn main() {
    //Get what version we are
    let version = env!("CARGO_PKG_VERSION");
    println!("Version: {}", version);
    println!("--------------------");

    //Configuration
    let mut helipad_config = HelipadConfig {
        database_file_path: "".to_string(),
        listen_port: "".to_string(),
        macaroon_path: "".to_string(),
        cert_path: "".to_string(),
    };

    //Bring in the configuration info
    let (server_config, _remaining_args) = Config::including_optional_config_files(&[HELIPAD_CONFIG_FILE]).unwrap_or_exit();

    //Debugging
    println!("Config file(database_dir): {:#?}", server_config.database_dir);
    println!("Config file(listen_port): {:#?}", server_config.listen_port);
    println!("Config file(macaroon): {:#?}", server_config.macaroon);
    println!("Config file(cert): {:#?}", server_config.cert);

    //LISTEN PORT -----
    println!("\nDiscovering listen port...");
    let mut listen_port = String::from(HELIPAD_STANDARD_PORT);
    let args: Vec<String> = env::args().collect();
    let env_listen_port = std::env::var("HELIPAD_LISTEN_PORT");
    //First try from the environment
    if env_listen_port.is_ok() {
        listen_port = env_listen_port.unwrap();
        println!(" - Using environment var(HELIPAD_LISTEN_PORT): [{}]", listen_port);
    } else if server_config.listen_port.is_some() {
        //If that fails, try from the config file
        listen_port = server_config.listen_port.unwrap().to_string();
        println!(" - Using config file({}): [{}]", HELIPAD_CONFIG_FILE, listen_port);
    } else if let Some(arg_port) = args.get(1) {
        //If that fails, try from the command line
        listen_port = arg_port.to_owned();
        println!(" - Using arg from command line: [{}]", listen_port);
    } else {
        //If everything fails, then just use the default port
        println!(" - Nothing else found. Using default: [{}]...", listen_port);
    }
    helipad_config.listen_port = listen_port.clone();

    //DATABASE FILE -----
    //First try to get the database file location from the environment
    println!("\nDiscovering database location...");
    let env_database_file_path = std::env::var("HELIPAD_DATABASE_DIR");
    if env_database_file_path.is_ok() {
        helipad_config.database_file_path = env_database_file_path.unwrap();
        println!(" - Using environment var(HELIPAD_DATABASE_DIR): [{}]", helipad_config.database_file_path);
    } else {
        //If that fails, try to get it from the config file
        if server_config.database_dir.is_some() {
            helipad_config.database_file_path = server_config.database_dir.clone().unwrap().to_string();
            println!(" - Using config file({}): [{}]", HELIPAD_CONFIG_FILE, helipad_config.database_file_path);
        } else {
            //If that fails just fall back to the local directory
            helipad_config.database_file_path = HELIPAD_DATABASE_DIR.to_string();
            println!(" - Nothing else found. Using default: [{}]", helipad_config.database_file_path);
        }
    }
    //Create the database file
    match dbif::create_database(&helipad_config.database_file_path) {
        Ok(_) => {
            println!("Database file is ready...");
        }
        Err(e) => {
            eprintln!("Database error: {:#?}", e);
            std::process::exit(3);
        }
    }

    //Start the LND polling thread.  This thread will poll LND every few seconds to
    //get the latest invoices and store them in the database.
    tokio::spawn(lnd_poller(server_config, helipad_config.database_file_path.clone()));

    //Router
    let some_state = "state".to_string();
    let mut router: Router = Router::new();

    //Base
    router.get("/", Box::new(handler::home));
    router.get("/pew.mp3", Box::new(handler::pewmp3));
    router.get("/favicon.ico", Box::new(handler::favicon));
    //Assets
    router.get("/image", Box::new(handler::asset));
    router.get("/html", Box::new(handler::asset));
    router.get("/style", Box::new(handler::asset));
    router.get("/script", Box::new(handler::asset));
    router.get("/extra", Box::new(handler::asset));
    //Api
    router.get("/boosts", Box::new(handler::boosts));
    //router.get("/streams", Box::new(handler::streams));

    let shared_router = Arc::new(router);
    let db_filepath: String = helipad_config.database_file_path.clone();
    let new_service = make_service_fn(move |conn: &AddrStream| {
        let app_state = AppState {
            state_thing: some_state.clone(),
            remote_ip: conn.remote_addr().to_string().clone(),
        };

        let database_file_path = db_filepath.clone();

        let router_capture = shared_router.clone();
        async {
            Ok::<_, Error>(service_fn(move |req| {
                route(router_capture.clone(), req, app_state.clone(), database_file_path.clone())
            }))
        }
    });

    let binding = format!("0.0.0.0:{}", &listen_port);
    let addr = binding.parse().expect("address creation works");
    let server = Server::bind(&addr).serve(new_service);
    println!("\nHelipad is listening on http://{}", addr);

    //If a "run as" user is set in the "HELIPAD_RUN_AS" environment variable, then switch to that user
    //and drop root privileges after we've bound to the low range socket
    match env::var("HELIPAD_RUNAS_USER") {
        Ok(runas_user) => {
            match set_user_group(runas_user.as_str(), "nogroup") {
                Ok(_) => {
                    println!("RunAs: {}", runas_user.as_str());
                }
                Err(e) => {
                    eprintln!("RunAs Error: {} - Check that your HELIPAD_RUNAS_USER env var is set correctly.", e);
                }
            }
        }
        Err(_) => {
            eprintln!("ALERT: Use the HELIPAD_RUNAS_USER env var to avoid running as root.");
        }
    }

    let _ = server.await;
}

async fn route(
    router: Arc<Router>,
    req: Request<hyper::Body>,
    app_state: AppState,
    database_file_path: String,
) -> Result<Response, Error> {
    let found_handler = router.route(req.uri().path(), req.method());
    let path = req.uri().path().to_owned();
    let resp = found_handler
        .handler
        .invoke(Context::new(app_state, req, &path, found_handler.params, database_file_path))
        .await;
    Ok(resp)
}

impl Context {
    pub fn new(state: AppState, reqbody: Request<Body>, path: &str, params: Params, database_file_path: String) -> Context {
        Context {
            state: state,
            req: reqbody,
            path: path.to_string(),
            params: params,
            database_file_path: database_file_path,
            body_bytes: None,
        }
    }

    pub async fn body_json<T: serde::de::DeserializeOwned>(&mut self) -> Result<T, Error> {
        let body_bytes = match self.body_bytes {
            Some(ref v) => v,
            _ => {
                let body = to_bytes(self.req.body_mut()).await?;
                self.body_bytes = Some(body);
                self.body_bytes.as_ref().expect("body_bytes was set above")
            }
        };
        Ok(serde_json::from_slice(&body_bytes)?)
    }
}

//The LND poller runs in a thread and pulls new invoices
async fn lnd_poller(server_config: Config, database_file_path: String) {

    let db_filepath = database_file_path;

    //Get the macaroon and cert files.  Look in the local directory first as an override.
    //If the files are not found in the currect working directory, look for them at their
    //normal LND directory locations
    println!("\nDiscovering macaroon file path...");
    let macaroon_path;
    let env_macaroon_path = std::env::var("LND_ADMINMACAROON");
    //First try from the environment
    if env_macaroon_path.is_ok() {
        macaroon_path = env_macaroon_path.unwrap();
        println!(" - Trying environment var(LND_ADMINMACAROON): [{}]", macaroon_path);
    } else if server_config.macaroon.is_some() {
        macaroon_path = server_config.macaroon.unwrap();
        println!(" - Trying config file({}): [{}]", HELIPAD_CONFIG_FILE, macaroon_path);
    } else {
        macaroon_path = "admin.macaroon".to_string();
        println!(" - Trying current directory: [{}]", macaroon_path);
    }
    let macaroon: Vec<u8>;
    match fs::read(macaroon_path.clone()) {
        Ok(macaroon_content) => {
            println!(" - Success.");
            macaroon = macaroon_content;
        }
        Err(_) => {
            println!(" - Error reading macaroon from: [{}]", macaroon_path);
            println!(" - Last fallback attempt: [{}]", LND_STANDARD_MACAROON_LOCATION);
            match fs::read(LND_STANDARD_MACAROON_LOCATION) {
                Ok(macaroon_content) => {
                    macaroon = macaroon_content;
                }
                Err(_) => {
                    eprintln!("Cannot find a valid admin.macaroon file");
                    std::process::exit(1);
                }
            }
        }
    }

    println!("\nDiscovering certificate file path...");
    let cert_path;
    let env_cert_path = std::env::var("LND_TLSCERT");
    if env_cert_path.is_ok() {
        cert_path = env_cert_path.unwrap();
        println!(" - Trying environment var(LND_TLSCERT): [{}]", cert_path);
    } else if server_config.cert.is_some() {
        cert_path = server_config.cert.unwrap();
        println!(" - Trying config file({}): [{}]", HELIPAD_CONFIG_FILE, cert_path);
    } else {
        cert_path = "tls.cert".to_string();
        println!(" - Trying current directory: [{}]", cert_path);
    }
    let cert: Vec<u8>;
    match fs::read(cert_path.clone()) {
        Ok(cert_content) => {
            println!(" - Success.");
            cert = cert_content;
        }
        Err(_) => {
            println!(" - Error reading certificate from: [{}]", cert_path);
            println!(" - Last fallback attempt: [{}]", LND_STANDARD_TLSCERT_LOCATION);
            match fs::read(LND_STANDARD_TLSCERT_LOCATION) {
                Ok(cert_content) => {
                    cert = cert_content;
                }
                Err(_) => {
                    eprintln!("Cannot find a valid tls.cert file");
                    std::process::exit(2);
                }
            }
        }
    }

    //Get the url connection string of the lnd node
    println!("\nDiscovering LND node address...");
    let node_address;
    let env_lnd_url = std::env::var("LND_URL");
    if env_lnd_url.is_ok() {
        node_address = "https://".to_owned() + env_lnd_url.unwrap().as_str();
        println!(" - Trying environment var(LND_URL): [{}]", node_address);
    } else if server_config.lnd_url.is_some() {
        node_address = server_config.lnd_url.unwrap();
        println!(" - Trying config file({}): [{}]", HELIPAD_CONFIG_FILE, node_address);
    } else {
        node_address = String::from(LND_STANDARD_GRPC_URL);
        println!(" - Trying localhost default: [{}].", node_address);
    }

    //Make the connection to LND
    let mut lightning;
    match lnd::Lnd::connect_with_macaroon(node_address.clone(), &cert, &macaroon).await {
        Ok(lndconn) => {
            println!(" - Success.");
            lightning = lndconn;
        }
        Err(e) => {
            println!("Could not connect to: [{}] using tls: [{}] and macaroon: [{}]", node_address, cert_path, macaroon_path);
            eprintln!("{:#?}", e);
            std::process::exit(1);
        }
    }

    //The main loop
    let mut current_index = dbif::get_last_boost_index_from_db(&db_filepath).unwrap();
    loop {

        //Get a list of invoices
        match lnd::Lnd::list_invoices(&mut lightning, false, current_index.clone(), 500, false).await {
            Ok(response) => {
                for invoice in response.invoices {

                    //Initialize a boost record
                    let mut boost = dbif::BoostRecord {
                        index: invoice.add_index,
                        time: invoice.settle_date,
                        value_msat: invoice.amt_paid_sat * 1000,
                        value_msat_total: invoice.amt_paid_sat * 1000,
                        action: 0,
                        sender: "".to_string(),
                        app: "".to_string(),
                        message: "".to_string(),
                        podcast: "".to_string(),
                        episode: "".to_string(),
                        tlv: "".to_string(),
                    };

                    //Search for podcast boost tlvs
                    for htlc in invoice.htlcs {
                        for (idx, val) in htlc.custom_records {
                            //Satoshis.stream record type
                            if idx == 7629169 {
                                boost.tlv = std::str::from_utf8(&val).unwrap().to_string();
                                let tlv = std::str::from_utf8(&val).unwrap();
                                println!("TLV: {:#?}", tlv);
                                let json_result = serde_json::from_str::<RawBoost>(tlv);
                                match json_result {
                                    Ok(rawboost) => {
                                        println!("{:#?}", rawboost);
                                        //If there was a sat value in the tlv, override the invoice
                                        if rawboost.value_msat.is_some() {
                                            boost.value_msat = rawboost.value_msat.unwrap() as i64;
                                        }
                                        //Determine an action type for later filtering ability
                                        if rawboost.action.is_some() {
                                            boost.action = match rawboost.action.unwrap().as_str() {
                                                "stream" => 1, //This indicates a per-minute podcast payment
                                                "boost" => 2,  //This is a manual boost or boost-a-gram
                                                _ => 3,
                                            }
                                        }
                                        //Was a sender name given in the tlv?
                                        if rawboost.sender_name.is_some() && !rawboost.sender_name.clone().unwrap().is_empty() {
                                            boost.sender = rawboost.sender_name.unwrap();
                                        }
                                        //Was there a message in this tlv?
                                        if rawboost.message.is_some() {
                                            boost.message = rawboost.message.unwrap();
                                        }
                                        //Was an app name given?
                                        if rawboost.app_name.is_some() {
                                            boost.app = rawboost.app_name.unwrap();
                                        }
                                        //Was a podcast name given?
                                        if rawboost.podcast.is_some() {
                                            boost.podcast = rawboost.podcast.unwrap();
                                        }
                                        //Episode name?
                                        if rawboost.episode.is_some() {
                                            boost.episode = rawboost.episode.unwrap();
                                        }
                                        //Look for an original sat value in the tlv
                                        if rawboost.value_msat_total.is_some() {
                                            boost.value_msat_total = rawboost.value_msat_total.unwrap() as i64;
                                        }
                                    }
                                    Err(e) => {
                                        eprintln!("{}", e);
                                    }
                                }
                            }
                        }
                    }

                    //Give some output
                    println!("Boost: {:#?}", boost);

                    //Store in the database
                    println!("{:#?}", boost);
                    match dbif::add_invoice_to_db(&db_filepath, boost) {
                        Ok(_) => println!("New invoice added."),
                        Err(e) => eprintln!("Error adding invoice: {:#?}", e)
                    }
                }
            }
            Err(e) => {
                eprintln!("{}", e);
            }
        }

        //Make sure we are tracking our position properly
        current_index = dbif::get_last_boost_index_from_db(&db_filepath).unwrap();
        println!("Current index: {}", current_index);

        std::thread::sleep(std::time::Duration::from_millis(9000));
    }
}