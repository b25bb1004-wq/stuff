//! Demo: run a short SQL script on an in-memory Iridium database and print the results.
//!
//! Run with `cargo run -p iridium --example demo`. No files, network or clock are used,
//! so the output is the same on every run.

use iridium::sql::{Db, Value};
use iridium::{Error, MemStorage};

/// Run one statement, then print its echo and either its rows or its change count.
fn show(db: &mut Db<MemStorage>, sql: &str) -> Result<(), Error> {
    let rows = db.execute(sql)?;
    println!("> {sql}");
    if rows.columns.is_empty() {
        println!("  ok, {} row(s) changed", rows.changes);
        return Ok(());
    }
    println!("  {}", rows.columns.join(" | "));
    for row in &rows.rows {
        let cells: Vec<String> = row.iter().map(cell).collect();
        println!("  {}", cells.join(" | "));
    }
    Ok(())
}

/// Render one SQL value as text for the printed table.
fn cell(value: &Value) -> String {
    match value {
        Value::Null => "NULL".to_string(),
        Value::Int(n) => n.to_string(),
        Value::Real(f) => f.to_string(),
        Value::Text(s) => s.clone(),
        Value::Blob(b) => format!("blob({} bytes)", b.len()),
    }
}

fn main() -> Result<(), Error> {
    // Both stores live in memory: the first holds pages, the second holds the write-ahead log.
    let mut db = Db::open(MemStorage::new(), MemStorage::new())?;

    show(
        &mut db,
        "CREATE TABLE city (id INTEGER PRIMARY KEY, name TEXT NOT NULL, pop INTEGER)",
    )?;
    show(
        &mut db,
        "INSERT INTO city (name, pop) VALUES ('Oslo', 709037), ('Lima', 9751717), ('Pune', 3124458)",
    )?;

    // A transaction that is rolled back leaves no trace.
    show(&mut db, "BEGIN")?;
    println!("  in transaction: {}", db.in_transaction());
    show(&mut db, "UPDATE city SET pop = pop + 1 WHERE name = 'Oslo'")?;
    show(&mut db, "ROLLBACK")?;
    println!("  in transaction: {}", db.in_transaction());

    show(&mut db, "SELECT name, pop FROM city ORDER BY pop DESC")?;
    show(&mut db, "DELETE FROM city WHERE pop < 1000000")?;
    show(&mut db, "SELECT COUNT(*) AS n FROM city")?;

    // A duplicate primary key is refused and the table is left unchanged.
    match db.execute("INSERT INTO city (id, name, pop) VALUES (2, 'Dup', 1)") {
        Ok(_) => println!("unexpected: duplicate key accepted"),
        Err(e) => println!("> duplicate primary key refused: {e}"),
    }

    let (_main, _log) = db.close()?;
    println!("database closed cleanly");
    Ok(())
}
