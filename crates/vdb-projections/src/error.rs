use vdb_types::Offset;

#[derive(thiserror::Error, Debug)]
pub enum ProjectionError {
    #[error(transparent)]
    Database(#[from] sqlx::Error),
    #[error("checkpoint mismatch. expected {expected}, received {actual} ")]
    CheckpointMismatch { expected: Offset, actual: Offset },
}
