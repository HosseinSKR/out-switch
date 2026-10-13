use anyhow::Result;
use outswitch_common::*;
use rusqlite::{params, Connection};
use std::path::Path;

pub struct Store(Connection);
impl Store {
    pub fn open(path: &Path) -> Result<Self> {
        if let Some(parent) = path.parent() { std::fs::create_dir_all(parent)?; }
        let conn = Connection::open(path)?;
        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.execute_batch("CREATE TABLE IF NOT EXISTS config (id INTEGER PRIMARY KEY CHECK(id=1), json TEXT NOT NULL);
            CREATE TABLE IF NOT EXISTS usage (day INTEGER NOT NULL, network TEXT NOT NULL, downloaded INTEGER NOT NULL, uploaded INTEGER NOT NULL, disconnects INTEGER NOT NULL, PRIMARY KEY(day,network));
            PRAGMA user_version=1;")?;
        Ok(Self(conn))
    }
    pub fn load(&self) -> Result<Config> {
        use rusqlite::OptionalExtension;
        let json: Option<String> = self.0.query_row("SELECT json FROM config WHERE id=1", [], |r| r.get(0)).optional()?;
        match json { Some(s) => Ok(serde_json::from_str(&s)?), None => Ok(Config::default()) }
    }
    pub fn save(&self, config: &Config) -> Result<()> {
        self.0.execute("INSERT INTO config(id,json) VALUES(1,?1) ON CONFLICT(id) DO UPDATE SET json=excluded.json", [serde_json::to_string(config)?])?; Ok(())
    }
    pub fn record(&self, usage: &Usage) -> Result<()> {
        self.0.execute("INSERT INTO usage VALUES(?1,?2,?3,?4,?5) ON CONFLICT(day,network) DO UPDATE SET downloaded=downloaded+excluded.downloaded,uploaded=uploaded+excluded.uploaded,disconnects=disconnects+excluded.disconnects", params![usage.day, usage.network, usage.downloaded, usage.uploaded, usage.disconnects])?;
        self.0.execute("DELETE FROM usage WHERE day < ?1", [now()/86400-90])?; Ok(())
    }
    pub fn usage(&self) -> Result<Vec<Usage>> {
        let mut stmt = self.0.prepare("SELECT day,network,downloaded,uploaded,disconnects FROM usage ORDER BY day DESC")?;
        let rows = stmt.query_map([], |r| Ok(Usage { day:r.get(0)?,network:r.get(1)?,downloaded:r.get(2)?,uploaded:r.get(3)?,disconnects:r.get(4)? }))?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }
}
#[cfg(test)] mod tests {
    use super::*;
    #[test] fn roundtrip_profiles_and_totals() {
        let store = Store(Connection::open_in_memory().unwrap());
        store.0.execute_batch("CREATE TABLE config(id INTEGER PRIMARY KEY,json TEXT); CREATE TABLE usage(day INTEGER,network TEXT,downloaded INTEGER,uploaded INTEGER,disconnects INTEGER,PRIMARY KEY(day,network));").unwrap();
        let mut cfg = Config::default(); cfg.profiles.get_mut("Gaming").unwrap().push(Rule::automatic(r"C:\Game.exe".into()));
        store.save(&cfg).unwrap(); assert_eq!(store.load().unwrap().profiles["Gaming"].len(),1);
        let u = Usage{day:now()/86400,network:"wifi".into(),downloaded:200,uploaded:30,disconnects:1};
        store.record(&u).unwrap(); store.record(&u).unwrap(); let rows=store.usage().unwrap();
        assert_eq!(rows[0].downloaded,400); assert_eq!(rows[0].disconnects,2);
    }
}
