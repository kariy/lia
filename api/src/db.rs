use sqlx::PgPool;
use uuid::Uuid;

use crate::error::{ApiError, ApiResult};
use crate::models::{GuildTask, Task, TaskConfig, TaskSource, TaskStatus};

pub async fn create_task(
    pool: &PgPool,
    user_id: &str,
    source: TaskSource,
    repositories: &[String],
    config: Option<TaskConfig>,
) -> ApiResult<Task> {
    let id = Uuid::new_v4();
    let config_json = config.map(sqlx::types::Json);

    let task = sqlx::query_as::<_, Task>(
        r#"
        INSERT INTO tasks (id, user_id, status, source, repositories, config, created_at)
        VALUES ($1, $2, $3, $4, $5, $6, NOW())
        RETURNING *
        "#,
    )
    .bind(id)
    .bind(user_id)
    .bind(TaskStatus::Pending)
    .bind(source)
    .bind(repositories)
    .bind(config_json)
    .fetch_one(pool)
    .await?;

    Ok(task)
}

pub async fn create_guild_task(
    pool: &PgPool,
    task_id: Uuid,
    guild_id: &str,
) -> ApiResult<GuildTask> {
    let guild_task = sqlx::query_as::<_, GuildTask>(
        r#"
        INSERT INTO guild_tasks (task_id, guild_id, created_at)
        VALUES ($1, $2, NOW())
        RETURNING *
        "#,
    )
    .bind(task_id)
    .bind(guild_id)
    .fetch_one(pool)
    .await?;

    Ok(guild_task)
}

pub async fn get_guild_id_for_task(pool: &PgPool, task_id: Uuid) -> ApiResult<Option<String>> {
    let result: Option<(String,)> = sqlx::query_as(
        "SELECT guild_id FROM guild_tasks WHERE task_id = $1",
    )
    .bind(task_id)
    .fetch_optional(pool)
    .await?;

    Ok(result.map(|(guild_id,)| guild_id))
}

pub async fn get_task(pool: &PgPool, id: Uuid) -> ApiResult<Task> {
    let task = sqlx::query_as::<_, Task>("SELECT * FROM tasks WHERE id = $1")
        .bind(id)
        .fetch_optional(pool)
        .await?
        .ok_or_else(|| ApiError::TaskNotFound(id.to_string()))?;

    Ok(task)
}

pub async fn list_tasks(
    pool: &PgPool,
    user_id: Option<&str>,
    status: Option<TaskStatus>,
    page: u32,
    per_page: u32,
) -> ApiResult<(Vec<Task>, i64)> {
    let offset = (page.saturating_sub(1)) * per_page;

    let tasks = sqlx::query_as::<_, Task>(
        r#"
        SELECT * FROM tasks
        WHERE ($1::VARCHAR IS NULL OR user_id = $1)
          AND ($2::VARCHAR IS NULL OR status = $2)
        ORDER BY created_at DESC
        LIMIT $3 OFFSET $4
        "#,
    )
    .bind(user_id)
    .bind(status.map(|s| s.to_string()))
    .bind(per_page as i64)
    .bind(offset as i64)
    .fetch_all(pool)
    .await?;

    let total: (i64,) = sqlx::query_as(
        r#"
        SELECT COUNT(*) FROM tasks
        WHERE ($1::VARCHAR IS NULL OR user_id = $1)
          AND ($2::VARCHAR IS NULL OR status = $2)
        "#,
    )
    .bind(user_id)
    .bind(status.map(|s| s.to_string()))
    .fetch_one(pool)
    .await?;

    Ok((tasks, total.0))
}

pub async fn update_task_status(
    pool: &PgPool,
    id: Uuid,
    status: TaskStatus,
    vm_id: Option<&str>,
) -> ApiResult<Task> {
    let task = sqlx::query_as::<_, Task>(
        r#"
        UPDATE tasks
        SET status = $2,
            vm_id = COALESCE($3, vm_id),
            started_at = CASE WHEN $2 = 'running' AND started_at IS NULL THEN NOW() ELSE started_at END
        WHERE id = $1
        RETURNING *
        "#,
    )
    .bind(id)
    .bind(status)
    .bind(vm_id)
    .fetch_optional(pool)
    .await?
    .ok_or_else(|| ApiError::TaskNotFound(id.to_string()))?;

    Ok(task)
}

pub async fn update_task_ip_address(
    pool: &PgPool,
    id: Uuid,
    ip_address: &str,
) -> ApiResult<Task> {
    let task = sqlx::query_as::<_, Task>(
        r#"
        UPDATE tasks
        SET ip_address = $2
        WHERE id = $1
        RETURNING *
        "#,
    )
    .bind(id)
    .bind(ip_address)
    .fetch_optional(pool)
    .await?
    .ok_or_else(|| ApiError::TaskNotFound(id.to_string()))?;

    Ok(task)
}

pub async fn complete_task(
    pool: &PgPool,
    id: Uuid,
    exit_code: i32,
    error_message: Option<&str>,
) -> ApiResult<Task> {
    let status = if exit_code == 0 {
        TaskStatus::Terminated
    } else {
        TaskStatus::Terminated
    };

    let task = sqlx::query_as::<_, Task>(
        r#"
        UPDATE tasks
        SET status = $2,
            exit_code = $3,
            error_message = $4,
            completed_at = NOW()
        WHERE id = $1
        RETURNING *
        "#,
    )
    .bind(id)
    .bind(status)
    .bind(exit_code)
    .bind(error_message)
    .fetch_optional(pool)
    .await?
    .ok_or_else(|| ApiError::TaskNotFound(id.to_string()))?;

    Ok(task)
}

pub async fn delete_task(pool: &PgPool, id: Uuid) -> ApiResult<()> {
    let result = sqlx::query("DELETE FROM tasks WHERE id = $1")
        .bind(id)
        .execute(pool)
        .await?;

    if result.rows_affected() == 0 {
        return Err(ApiError::TaskNotFound(id.to_string()));
    }

    Ok(())
}
