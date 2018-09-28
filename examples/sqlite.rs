#[macro_use]
extern crate finchers;
extern crate finchers_r2d2 as r2d2;
#[macro_use]
extern crate log;
extern crate pretty_env_logger;
extern crate r2d2_sqlite;
extern crate rusqlite;
#[macro_use]
extern crate serde;

use finchers::prelude::*;

use r2d2::{Pool, PooledConnection};
use r2d2_sqlite::SqliteConnectionManager;

type Conn = PooledConnection<SqliteConnectionManager>;

#[derive(Debug, Serialize)]
struct Person {
    id: i32,
    name: String,
    data: Option<Vec<u8>>,
}

impl Person {
    fn create_table(conn: &rusqlite::Connection) -> rusqlite::Result<()> {
        conn.execute(
            "CREATE TABLE person (\
             id      INTEGER PRIMARY KEY, \
             name    TEXT NOT NULL, \
             data    BLOB\
             )",
            &[],
        ).map(|_| ())
    }

    fn insert_into(self, conn: &rusqlite::Connection) -> rusqlite::Result<()> {
        conn.execute(
            "INSERT INTO person (name, data) \
             VALUES (?1, ?2)",
            &[&self.name, &self.data],
        ).map(|_| ())
    }

    fn select_all(conn: &rusqlite::Connection) -> rusqlite::Result<Vec<Person>> {
        let mut stmt = conn.prepare("SELECT id,name,data FROM person")?;
        let iter = stmt.query_map(&[], |row| Person {
            id: row.get(0),
            name: row.get(1),
            data: row.get(2),
        })?;
        iter.collect()
    }
}

fn main() {
    pretty_env_logger::init();

    let manager = SqliteConnectionManager::file("app.db");
    let pool = Pool::builder()
        .max_size(1)
        .build(manager)
        .expect("failed to build the connection pool");
    {
        let conn = pool.get().unwrap();
        Person::create_table(&*conn).unwrap();

        (Person {
            id: 0,
            name: "Alice".to_owned(),
            data: None,
        }).insert_into(&*conn)
        .unwrap();
    }
    let retrieve_conn = r2d2::pool_endpoint(pool);

    let endpoint = path!(@get / "person")
        .and(retrieve_conn)
        .and_then(|conn: Conn| {
            Person::select_all(&*conn)
                .map(finchers::output::Json)
                .map_err(finchers::error::fail)
        });

    info!("Listening on http://127.0.0.1:4000");
    finchers::launch(endpoint).start("127.0.0.1:4000")
}
