use clap::Subcommand;
use rand::{Rng, distr::Alphanumeric};
use sqlx::{Pool, Sqlite, types::time::UtcOffset};
use time::macros::format_description;

use crate::{CLINET_TOKEN_LENGTH, index_client_token};

#[derive(Debug, Subcommand)]
pub enum AdminCommands {
    /// User related commands
    #[command(subcommand)]
    Client(ClientCommands),
}

#[derive(Debug, Subcommand)]
pub enum ClientCommands {
    /// List all clients
    #[clap(visible_alias("ls"))]
    List,
    /// Add a new client
    #[clap(visible_alias("a"))]
    Add { username: String },
    /// Remove a client
    #[clap(visible_alias("rm"))]
    Remove { id: i64 },
    /// Rename a client
    Rename { id: i64, new_username: String },
}

pub async fn admin(command: AdminCommands, pool: Pool<Sqlite>) -> anyhow::Result<()> {
    match command {
        AdminCommands::Client(client_command) => match client_command {
            ClientCommands::List => list_clients(&pool).await,
            ClientCommands::Add { username } => add_client(&pool, username).await,
            ClientCommands::Rename { id, new_username } => {
                rename_client(&pool, id, new_username).await
            }
            ClientCommands::Remove { id } => remove_client(&pool, id).await,
        },
    }
}

async fn list_clients(pool: &Pool<Sqlite>) -> anyhow::Result<()> {
    let clients = sqlx::query!("SELECT id,name,created_at FROM clients")
        .fetch_all(pool)
        .await?;

    for client in clients {
        println!(
            "[{}] {} (created at: {})",
            client.id,
            client.name,
            client
                .created_at
                .to_offset(UtcOffset::current_local_offset().unwrap_or(UtcOffset::UTC))
                .format(format_description!(
                    "[year]-[month]-[day] [hour]:[minute]:[second]"
                ))
                .unwrap()
        );
    }

    Ok(())
}

async fn add_client(pool: &Pool<Sqlite>, username: String) -> anyhow::Result<()> {
    let mut tx = pool.begin().await?;

    // Ensure the token is unique
    let (token, token_idx, token_hash) = loop {
        let token: String = rand::rng()
            .sample_iter(&Alphanumeric)
            .take(CLINET_TOKEN_LENGTH)
            .map(char::from)
            .collect();

        let token_idx = index_client_token(&token);
        let token_hash = password_auth::generate_hash(&token);

        if sqlx::query!("SELECT id FROM clients WHERE token_hash = ?", token_hash)
            .fetch_optional(&mut *tx)
            .await?
            .is_none()
        {
            break (token, token_idx, token_hash);
        }
    };

    let record = sqlx::query!(
        "INSERT INTO clients (name, token_idx, token_hash) VALUES (?, ?, ?) RETURNING id",
        username,
        token_idx,
        token_hash
    )
    .fetch_one(&mut *tx)
    .await?;

    tx.commit().await?;

    println!("Client '{}' [{}] added successfully.", username, record.id);
    println!("Token: {token}");
    Ok(())
}

async fn remove_client(pool: &Pool<Sqlite>, id: i64) -> anyhow::Result<()> {
    let rows_affected = sqlx::query!("DELETE FROM clients WHERE id = ?", id)
        .execute(pool)
        .await?
        .rows_affected();

    if rows_affected == 0 {
        println!("No client found with ID {id}.");
    } else {
        println!("Client with ID {id} removed successfully.");
    }

    Ok(())
}

async fn rename_client(pool: &Pool<Sqlite>, id: i64, new_username: String) -> anyhow::Result<()> {
    let rows_affected = sqlx::query!("UPDATE clients SET name = ? WHERE id = ?", new_username, id)
        .execute(pool)
        .await?
        .rows_affected();

    if rows_affected == 0 {
        println!("No client found with ID {id}.");
    } else {
        println!("Client with ID {id} renamed successfully.");
    }

    Ok(())
}
