use rusqlite::{Connection, Result};
use std::error::Error;
use wasm_bindgen::prelude::*;

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
pub fn run_sqlite_demo() -> Result<(), JsValue> {
    // Set up panic hook for better error messages in the browser
    console_error_panic_hook::set_once();
    console_log::init_with_level(log::Level::Trace).unwrap();

    // Open an in-memory database
    let conn = Connection::open_in_memory().map_err(|e| ErrorShim::from(e))?;

    conn.execute(
        "CREATE TABLE person (
            id    INTEGER PRIMARY KEY,
            name  TEXT NOT NULL,
            data  BLOB
        )",
        (),
    )
    .map_err(|e| ErrorShim::from(e))?;

    let me = Person {
        id: 0,
        name: "Steven".to_string(),
        data: None,
    };

    conn.execute(
        "INSERT INTO person (name, data) VALUES (?1, ?2)",
        (&me.name, &me.data),
    )
    .map_err(|e| ErrorShim::from(e))?;

    let mut stmt = conn
        .prepare("SELECT id, name, data FROM person")
        .map_err(|e| ErrorShim::from(e))?;
    let person_iter = stmt
        .query_map([], |row| {
            Ok(Person {
                id: row.get(0)?,
                name: row.get(1)?,
                data: row.get(2)?,
            })
        })
        .map_err(|e| ErrorShim::from(e))?;

    for person in person_iter {
        web_sys::console::log_1(&format!("Found person {:?}", person.unwrap()).into());
    }

    Ok(())
}
