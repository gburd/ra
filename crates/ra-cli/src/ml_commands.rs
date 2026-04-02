//! ML model management commands.

use std::path::PathBuf;

use anyhow::{Context, Result};
use clap::Subcommand;
use colored::Colorize;

use ra_ml::belief_network::BeliefNetwork;
use ra_ml::estimator::MlEstimator;
use ra_ml::features::FeatureSchema;
use ra_ml::nn::{build_default_mlp, FeedForwardNet};
use ra_ml::storage::{DatabaseBackend, ModelStorage, StorageConfig};
use ra_ml::streaming::{ModelScope, StreamingConfig, StreamingMlEstimator};
use ra_ml::training::TrainingDataset;

#[derive(Subcommand)]
pub enum MlCommands {
    /// Train a new ML model from a dataset.
    #[command(
        long_about = "Train a new cardinality estimation model from a JSON training dataset.\n\n\
            The training dataset should contain SQL queries with known cardinalities.\n\n\
            Examples:\n  \
            ra-cli ml train --dataset training.json --output model.json\n  \
            ra-cli ml train --dataset data/ --tables users,orders --columns id,name"
    )]
    Train {
        /// Path to training dataset (JSON file or directory).
        #[arg(short, long)]
        dataset: PathBuf,

        /// Output path for trained model.
        #[arg(short, long)]
        output: Option<PathBuf>,

        /// Table names (comma-separated).
        #[arg(short, long)]
        tables: Option<String>,

        /// Column names (comma-separated).
        #[arg(short, long)]
        columns: Option<String>,

        /// Save to database instead of file.
        #[arg(long)]
        database: bool,

        /// Model name for database storage.
        #[arg(long, default_value = "default")]
        model_name: String,
    },

    /// Load an ML model from the database.
    #[command(
        long_about = "Load a trained ML model from the database.\n\n\
            Examples:\n  \
            ra-cli ml load --name production_model --scope overall\n  \
            ra-cli ml load --name account_model --scope account --account-id abc123"
    )]
    Load {
        /// Model name to load.
        #[arg(short, long)]
        name: String,

        /// Model scope (account, project, or overall).
        #[arg(short, long, default_value = "overall")]
        scope: String,

        /// Account ID for account-scoped models.
        #[arg(long)]
        account_id: Option<String>,

        /// Project ID for project-scoped models.
        #[arg(long)]
        project_id: Option<String>,

        /// Database connection string.
        #[arg(long, default_value = "sqlite::memory:")]
        database: String,
    },

    /// Save an ML model to the database.
    #[command(
        long_about = "Save a trained ML model to the database.\n\n\
            Examples:\n  \
            ra-cli ml save --input model.json --name production_model\n  \
            ra-cli ml save --input model.json --name account_model --scope account"
    )]
    Save {
        /// Input model file (JSON).
        #[arg(short, long)]
        input: PathBuf,

        /// Model name for storage.
        #[arg(short, long)]
        name: String,

        /// Model scope (account, project, or overall).
        #[arg(short, long, default_value = "overall")]
        scope: String,

        /// Account ID for account-scoped models.
        #[arg(long)]
        account_id: Option<String>,

        /// Project ID for project-scoped models.
        #[arg(long)]
        project_id: Option<String>,

        /// Database connection string.
        #[arg(long, default_value = "sqlite:ra-ml.db")]
        database: String,

        /// Database backend (postgres or sqlite).
        #[arg(long, default_value = "sqlite")]
        backend: String,
    },

    /// Show ML model statistics and accuracy metrics.
    #[command(
        long_about = "Display statistics about trained ML models and belief networks.\n\n\
            Shows rule effectiveness, observation counts, improvement probabilities,\n\
            and q-error metrics for cardinality estimates.\n\n\
            Examples:\n  \
            ra-cli ml stats --name production_model\n  \
            ra-cli ml stats --name production_model --rule filter-pushdown"
    )]
    Stats {
        /// Model name to analyze.
        #[arg(short, long)]
        name: String,

        /// Show statistics for specific rule only.
        #[arg(short, long)]
        rule: Option<String>,

        /// Model scope (account, project, or overall).
        #[arg(short, long, default_value = "overall")]
        scope: String,

        /// Account ID for account-scoped models.
        #[arg(long)]
        account_id: Option<String>,

        /// Project ID for project-scoped models.
        #[arg(long)]
        project_id: Option<String>,

        /// Database connection string.
        #[arg(long, default_value = "sqlite:ra-ml.db")]
        database: String,

        /// Database backend (postgres or sqlite).
        #[arg(long, default_value = "sqlite")]
        backend: String,
    },

    /// Export ML model and belief network for external analysis.
    #[command(
        long_about = "Export ML models and belief networks to JSON for external analysis.\n\n\
            Examples:\n  \
            ra-cli ml export --name production_model --output export.json\n  \
            ra-cli ml export --name production_model --format csv --output stats.csv"
    )]
    Export {
        /// Model name to export.
        #[arg(short, long)]
        name: String,

        /// Output file path.
        #[arg(short, long)]
        output: PathBuf,

        /// Export format (json or csv).
        #[arg(short, long, default_value = "json")]
        format: String,

        /// Model scope (account, project, or overall).
        #[arg(short, long, default_value = "overall")]
        scope: String,

        /// Account ID for account-scoped models.
        #[arg(long)]
        account_id: Option<String>,

        /// Project ID for project-scoped models.
        #[arg(long)]
        project_id: Option<String>,

        /// Database connection string.
        #[arg(long, default_value = "sqlite:ra-ml.db")]
        database: String,

        /// Database backend (postgres or sqlite).
        #[arg(long, default_value = "sqlite")]
        backend: String,
    },
}

pub async fn handle_ml_command(cmd: MlCommands) -> Result<()> {
    match cmd {
        MlCommands::Train {
            dataset,
            output,
            tables,
            columns,
            database,
            model_name,
        } => train_model(dataset, output, tables, columns, database, model_name).await,
        MlCommands::Load {
            name,
            scope,
            account_id,
            project_id,
            database,
        } => load_model(&name, &scope, account_id.as_deref(), project_id.as_deref(), &database).await,
        MlCommands::Save {
            input,
            name,
            scope,
            account_id,
            project_id,
            database,
            backend,
        } => {
            save_model(
                &input,
                &name,
                &scope,
                account_id.as_deref(),
                project_id.as_deref(),
                &database,
                &backend,
            )
            .await
        }
        MlCommands::Stats {
            name,
            rule,
            scope,
            account_id,
            project_id,
            database,
            backend,
        } => {
            show_stats(
                &name,
                rule.as_deref(),
                &scope,
                account_id.as_deref(),
                project_id.as_deref(),
                &database,
                &backend,
            )
            .await
        }
        MlCommands::Export {
            name,
            output,
            format,
            scope,
            account_id,
            project_id,
            database,
            backend,
        } => {
            export_model(
                &name,
                &output,
                &format,
                &scope,
                account_id.as_deref(),
                project_id.as_deref(),
                &database,
                &backend,
            )
            .await
        }
    }
}

async fn train_model(
    _dataset: PathBuf,
    _output: Option<PathBuf>,
    tables: Option<String>,
    columns: Option<String>,
    _database: bool,
    _model_name: String,
) -> Result<()> {
    println!("{}", "Training ML model...".bold());

    let table_names: Vec<&str> = tables
        .as_ref()
        .map(|s| s.split(',').collect())
        .unwrap_or_else(|| vec!["users", "orders"]);

    let column_names: Vec<&str> = columns
        .as_ref()
        .map(|s| s.split(',').collect())
        .unwrap_or_else(|| vec!["id", "name", "amount"]);

    let schema = FeatureSchema::new(&table_names, &column_names);
    let model = build_default_mlp(&[schema.total_features, 64, 32, 1]);

    println!(
        "{}",
        format!(
            "Created model with {} input features, {} layers",
            schema.total_features,
            model.num_layers()
        )
        .green()
    );

    println!("{}", "Training not yet implemented (requires external Python training)".yellow());
    println!("Use the exported training dataset with PyTorch or TensorFlow");

    Ok(())
}

async fn load_model(
    name: &str,
    scope: &str,
    account_id: Option<&str>,
    project_id: Option<&str>,
    database: &str,
) -> Result<()> {
    println!("{}", format!("Loading model '{name}' from database...").bold());

    let config = StorageConfig {
        backend: DatabaseBackend::Sqlite,
        connection_string: database.to_string(),
        max_connections: 10,
    };

    let storage = ModelStorage::new(config)
        .await
        .context("Failed to connect to database")?;

    let (model, schema_data) = storage
        .load_model(name)
        .await
        .context("Failed to load model")?;

    println!(
        "{}",
        format!(
            "Loaded model: {} inputs, {} outputs, {} layers",
            model.input_size(),
            model.output_size(),
            model.num_layers()
        )
        .green()
    );

    let belief_state = storage
        .load_belief_network(scope, account_id, project_id)
        .await;

    match belief_state {
        Ok(state) => {
            println!(
                "{}",
                format!("Loaded belief network with {} CPTs", state.cpts.len()).green()
            );
        }
        Err(_) => {
            println!("{}", "No belief network found for this scope".yellow());
        }
    }

    Ok(())
}

async fn save_model(
    input: &PathBuf,
    name: &str,
    scope: &str,
    account_id: Option<&str>,
    project_id: Option<&str>,
    database: &str,
    backend_str: &str,
) -> Result<()> {
    println!("{}", format!("Saving model '{name}' to database...").bold());

    let model_json = std::fs::read(input).context("Failed to read model file")?;

    let model: FeedForwardNet =
        serde_json::from_slice(&model_json).context("Failed to parse model JSON")?;

    let backend = backend_str
        .parse::<DatabaseBackend>()
        .context("Invalid backend")?;

    let config = StorageConfig {
        backend,
        connection_string: database.to_string(),
        max_connections: 10,
    };

    let storage = ModelStorage::new(config)
        .await
        .context("Failed to connect to database")?;

    let schema_json = serde_json::to_vec(&FeatureSchema::new(&["users"], &["id"]))
        .context("Failed to serialize schema")?;

    storage
        .save_model(name, &model, &schema_json, scope, account_id, project_id)
        .await
        .context("Failed to save model")?;

    println!("{}", format!("Model '{name}' saved successfully").green());

    Ok(())
}

async fn show_stats(
    name: &str,
    rule: Option<&str>,
    scope: &str,
    account_id: Option<&str>,
    project_id: Option<&str>,
    database: &str,
    backend_str: &str,
) -> Result<()> {
    println!("{}", format!("Model statistics for '{name}'").bold());
    println!();

    let backend = backend_str
        .parse::<DatabaseBackend>()
        .context("Invalid backend")?;

    let config = StorageConfig {
        backend,
        connection_string: database.to_string(),
        max_connections: 10,
    };

    let storage = ModelStorage::new(config)
        .await
        .context("Failed to connect to database")?;

    let belief_state = storage
        .load_belief_network(scope, account_id, project_id)
        .await
        .context("Failed to load belief network")?;

    let network = BeliefNetwork::new();
    network.import(belief_state);

    let stats = if let Some(rule_id) = rule {
        vec![network
            .rule_statistics(rule_id)
            .context("Rule not found")?]
    } else {
        network.all_statistics()
    };

    if stats.is_empty() {
        println!("{}", "No statistics available".yellow());
        return Ok(());
    }

    println!("{:<30} {:>10} {:>12} {:>12}", "Rule ID", "Obs Count", "Improvement", "Q-Error");
    println!("{}", "-".repeat(70));

    for stat in stats {
        println!(
            "{:<30} {:>10} {:>11.2}% {:>12}",
            stat.rule_id,
            stat.observation_count,
            stat.mean_improvement * 100.0,
            stat.mean_q_error
                .map(|e| format!("{e:.2}"))
                .unwrap_or_else(|| "N/A".to_string())
        );
    }

    Ok(())
}

async fn export_model(
    name: &str,
    output: &PathBuf,
    format: &str,
    scope: &str,
    account_id: Option<&str>,
    project_id: Option<&str>,
    database: &str,
    backend_str: &str,
) -> Result<()> {
    println!("{}", format!("Exporting model '{name}' to {format}...").bold());

    let backend = backend_str
        .parse::<DatabaseBackend>()
        .context("Invalid backend")?;

    let config = StorageConfig {
        backend,
        connection_string: database.to_string(),
        max_connections: 10,
    };

    let storage = ModelStorage::new(config)
        .await
        .context("Failed to connect to database")?;

    let (model, _schema_data) = storage
        .load_model(name)
        .await
        .context("Failed to load model")?;

    let belief_state = storage
        .load_belief_network(scope, account_id, project_id)
        .await
        .context("Failed to load belief network")?;

    match format {
        "json" => {
            let export_data = serde_json::json!({
                "model": model,
                "belief_network": belief_state,
            });

            std::fs::write(output, serde_json::to_string_pretty(&export_data)?)
                .context("Failed to write export file")?;
        }
        "csv" => {
            let network = BeliefNetwork::new();
            network.import(belief_state);

            let mut csv = String::from("rule_id,observation_count,improvement_prob,mean_improvement,std_improvement,mean_q_error\n");

            for stat in network.all_statistics() {
                csv.push_str(&format!(
                    "{},{},{},{},{},{}\n",
                    stat.rule_id,
                    stat.observation_count,
                    stat.prior_improvement_prob,
                    stat.mean_improvement,
                    stat.std_improvement,
                    stat.mean_q_error.unwrap_or(0.0)
                ));
            }

            std::fs::write(output, csv).context("Failed to write CSV file")?;
        }
        _ => anyhow::bail!("Unsupported format: {format}"),
    }

    println!("{}", format!("Exported to {}", output.display()).green());

    Ok(())
}
