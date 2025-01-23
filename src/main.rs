use postgres::{Client, NoTls};
use postgres::Error as PostgresError;
use std::net::{TcpListener, TcpStream};
use std::io::{Read, Write};
use std::env;
// use serde::{Serialize, Deserialize};

#[macro_use]
extern crate serde_derive;

// Model: User struct with id, name, email
#[derive(Serialize, Deserialize)]
struct User {
    id: Option<i32>,
    name: String,
    email: String,
}

// Constants
const OK_RESPONSE: &str = "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\n\r\n";
const NOT_FOUND: &str = "HTTP/1.1 404 NOT FOUND\r\n\r\n";
const INTERNAL_SERVER_ERROR: &str = "HTTP/1.1 500 INTERNAL SERVER ERROR\r\n\r\n";

// Main function
fn main() {
    // Get the database URL
    let db_url = get_db_url();

    // Set up the database
    if let Err(e) = set_database(&db_url) {
        println!("Error setting up database: {}", e);
        return;
    }

    // Start server
    let listener = TcpListener::bind("0.0.0.0:8080").unwrap();
    println!("Server started at port 8080");

    // Handle client connections
    for stream in listener.incoming() {
        match stream {
            Ok(stream) => {
                handle_client(stream, &db_url);
            }
            Err(e) => {
                println!("Error handling client: {}", e);
            }
        }
    }
}

// Handle client request
fn handle_client(mut stream: TcpStream, db_url: &str) {
    let mut buffer = [0; 1024];
    let mut request = String::new();
    match stream.read(&mut buffer) {
        Ok(size) => {
            request.push_str(String::from_utf8_lossy(&buffer[..size]).as_ref());

            let (status_line, content) = match &*request {
                r if request.starts_with("POST /users") => handle_post_request(r, db_url),
                r if request.starts_with("GET /users") => handle_get_request(r, db_url),
                r if request.starts_with("GET /users/all") => handle_get_all_requests(r, db_url),
                r if request.starts_with("PUT /users") => handle_put_request(r, db_url),
                r if request.starts_with("DELETE /users") => handle_delete_request(r, db_url),
                _ => (NOT_FOUND.to_string(), "Not found".to_string()),
            };
            stream.write_all(format!("{}{}", status_line, content).as_bytes()).unwrap();
        }
        Err(e) => {
            println!("Error reading from stream: {}", e);
        }
    }
}

// Controllers for HTTP requests

fn handle_post_request(request: &str, db_url: &str) -> (String, String) {
    match (get_user_request_body(&request), Client::connect(db_url, NoTls)) {
        (Ok(user), Ok(mut client)) => {
            client
                .execute(
                    "INSERT INTO users (name, email) VALUES ($1,$2)",
                    &[&user.name, &user.email],
                )
                .unwrap();
            (OK_RESPONSE.to_string(), "User created".to_string())
        }
        _ => (INTERNAL_SERVER_ERROR.to_string(), "Error occurred".to_string()),
    }
}

fn handle_get_request(request: &str, db_url: &str) -> (String, String) {
    match (get_id(&request).parse::<i32>(), Client::connect(db_url, NoTls)) {
        (Ok(id), Ok(mut client)) => match client.query_one("SELECT * FROM users WHERE id = $1", &[&id]) {
            Ok(row) => {
                let user = User {
                    id: row.get(0),
                    name: row.get(1),
                    email: row.get(2),
                };
                (OK_RESPONSE.to_string(), serde_json::to_string(&user).unwrap())
            }
            Err(e) => {
                // Log the error if the user is not found or any other query error occurs
                println!("Database query error: {}", e);
                (NOT_FOUND.to_string(), "User not found".to_string())
            }
        },
        (Err(e), _) => {
            // Handle the case where parsing the ID fails
            println!("Error parsing ID: {}", e);
            (INTERNAL_SERVER_ERROR.to_string(), "Invalid ID format".to_string())
        }
        (_, Err(e)) => {
            // Handle database connection failure
            println!("Database connection error: {}", e);
            (INTERNAL_SERVER_ERROR.to_string(), "Database connection error".to_string())
        }
    }
}

fn handle_get_all_requests(request: &str, db_url: &str) -> (String, String) {
    match Client::connect(db_url, NoTls) {
        Ok(mut client) => {
            let mut users = Vec::new();
            for row in client.query("SELECT * FROM users", &[]).unwrap() {
                users.push(User {
                    id: row.get(0),
                    name: row.get(1),
                    email: row.get(2),
                });
            }
            (OK_RESPONSE.to_string(), serde_json::to_string(&users).unwrap())
        }
        _ => (INTERNAL_SERVER_ERROR.to_string(), "Error occurred".to_string()),
    }
}

fn handle_put_request(request: &str, db_url: &str) -> (String, String) {
    match (
        get_id(&request).parse::<i32>(),
        get_user_request_body(&request),
        Client::connect(db_url, NoTls),
    ) {
        (Ok(id), Ok(user), Ok(mut client)) => {
            client
                .execute("UPDATE users SET name = $1, email = $2 WHERE id = $3", &[&user.name, &user.email, &id])
                .unwrap();
            (OK_RESPONSE.to_string(), "User updated".to_string())
        }
        _ => (INTERNAL_SERVER_ERROR.to_string(), "Error occurred".to_string()),
    }
}

fn handle_delete_request(request: &str, db_url: &str) -> (String, String) {
    match (get_id(&request).parse::<i32>(), Client::connect(db_url, NoTls)) {
        (Ok(id), Ok(mut client)) => {
            let rows_affected = client.execute("DELETE FROM users WHERE id = $1", &[&id]).unwrap();
            if rows_affected == 0 {
                return (NOT_FOUND.to_string(), "User not found".to_string());
            }
            (OK_RESPONSE.to_string(), "User deleted".to_string())
        }
        _ => (INTERNAL_SERVER_ERROR.to_string(), "Error occurred".to_string()),
    }
}

// Set up the database (initialize if needed)
fn set_database(db_url: &str) -> Result<(), PostgresError> {
    let mut client = Client::connect(db_url, NoTls)?;

    client.execute(
        "CREATE TABLE IF NOT EXISTS users (
            id SERIAL PRIMARY KEY,
            name VARCHAR NOT NULL,
            email VARCHAR NOT NULL
        )",
        &[],
    )?;
    Ok(())
}

// Get ID from request URL
fn get_id(request: &str) -> &str {
    request.split("/").nth(2).unwrap_or_default()
}

// Deserialize the user from the request body
fn get_user_request_body(request: &str) -> Result<User, serde_json::Error> {
    serde_json::from_str(request.split("\r\n\r\n").last().unwrap_or_default())
}

// Retrieve the database URL from the environment
fn get_db_url() -> String {
    env::var("DATABASE_URL").expect("DATABASE_URL environment variable not set")
}
