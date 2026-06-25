use rusqlite::{Connection, Result};
use wasm_bindgen::prelude::*;
use wasm_bindgen_futures::JsFuture;
use web_sys::{Request, RequestInit, Response, console::log_1, window};

// need to fetch db from site
// use bare api: https://rustwasm.app/en/learn/fetch-api
async fn fetch_db(path: &str) -> Result<JsValue, JsValue> {
    let mut opts = RequestInit::new();
    opts.set_method("GET");

    let request = Request::new_with_str_and_init(path, &opts)?;
    let window = window().unwrap();
    let resp: Response = JsFuture::from(window.fetch_with_request(&request))
        .await?
        .dyn_into()?;

    Ok(resp.array_buffer()?.await?)
}

#[derive(Debug)]
struct Person {
    id: i32,
    name: String,
    data: Option<Vec<u8>>,
}

struct ErrorShim(String);

impl From<rusqlite::Error> for ErrorShim {
    fn from(value: rusqlite::Error) -> Self {
        ErrorShim(format!("Error: {value}"))
    }
}

impl From<ErrorShim> for JsValue {
    fn from(value: ErrorShim) -> Self {
        JsValue::from_str(&value.0)
    }
}

#[wasm_bindgen]
pub async fn run_sqlite_demo() -> Result<(), JsValue> {
    // Set up panic hook for better error messages in the browser
    console_error_panic_hook::set_once();
    console_log::init_with_level(log::Level::Trace).unwrap();

    let db_bytes = fetch_db("polydb.sqlite3")
        .await
        .map_err(|e| ErrorShim::from(e))?;
    // Open an in-memory database
    let conn = Connection::open_in_memory().map_err(|e| ErrorShim::from(e))?;
    conn.restore_from_memory(&db_bytes);

    let mut stmt = conn
        .prepare(
            "SELECT
        name
    FROM
        sqlite_schema
    WHERE
        type ='table' AND
        name NOT LIKE 'sqlite_%';",
        )
        .map_err(|e| ErrorShim::from(e))?;
    let res = stmt
        .query_map([], |row| Ok(format!("{row:?}")))
        .map_err(|e| ErrorShim::from(e))?;
    for row in res {
        log_1(&JsValue::from_str(&row.unwrap()));
    }
    Ok(())
}
