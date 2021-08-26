use flowy_net::{errors::ServerError, response::FlowyResponse};

use crate::{
    entities::workspace::AppTable,
    sqlx_ext::{map_sqlx_error, SqlBuilder},
    workspace_service::view::read_views_belong_to_id,
};
use anyhow::Context;
use chrono::Utc;
use flowy_net::errors::invalid_params;
use flowy_user::entities::parser::UserId;
use flowy_workspace::{
    entities::{
        app::{
            parser::{AppDesc, AppId, AppName},
            App,
            RepeatedApp,
        },
        view::RepeatedView,
        workspace::parser::WorkspaceId,
    },
    protobuf::{CreateAppParams, QueryAppParams, UpdateAppParams},
};
use protobuf::Message;
use sqlx::{postgres::PgArguments, PgPool, Postgres, Transaction};
use uuid::Uuid;

pub(crate) async fn create_app(
    pool: &PgPool,
    params: CreateAppParams,
) -> Result<FlowyResponse, ServerError> {
    let color_bytes = params.get_color_style().write_to_bytes()?;
    let name = AppName::parse(params.name).map_err(invalid_params)?;
    let workspace_id = WorkspaceId::parse(params.workspace_id).map_err(invalid_params)?;
    let user_id = UserId::parse(params.user_id).map_err(invalid_params)?;
    let desc = AppDesc::parse(params.desc).map_err(invalid_params)?;

    let mut transaction = pool
        .begin()
        .await
        .context("Failed to acquire a Postgres connection to create app")?;

    let uuid = uuid::Uuid::new_v4();
    let time = Utc::now();

    let (sql, args) = SqlBuilder::create("app_table")
        .add_arg("id", uuid)
        .add_arg("workspace_id", workspace_id.as_ref())
        .add_arg("name", name.as_ref())
        .add_arg("description", desc.as_ref())
        .add_arg("color_style", color_bytes)
        .add_arg("modified_time", &time)
        .add_arg("create_time", &time)
        .add_arg("user_id", user_id.as_ref())
        .build()?;

    let _ = sqlx::query_with(&sql, args)
        .execute(&mut transaction)
        .await
        .map_err(map_sqlx_error)?;

    transaction
        .commit()
        .await
        .context("Failed to commit SQL transaction to create app.")?;

    let app = App {
        id: uuid.to_string(),
        workspace_id: workspace_id.as_ref().to_owned(),
        name: name.as_ref().to_string(),
        desc: desc.as_ref().to_string(),
        belongings: RepeatedView::default(),
        version: 0,
    };

    FlowyResponse::success().data(app)
}

pub(crate) async fn read_app(
    pool: &PgPool,
    params: QueryAppParams,
) -> Result<FlowyResponse, ServerError> {
    let app_id = check_app_id(params.app_id)?;

    let mut transaction = pool
        .begin()
        .await
        .context("Failed to acquire a Postgres connection to read app")?;

    let (sql, args) = SqlBuilder::select("app_table")
        .add_field("*")
        .and_where_eq("id", app_id)
        .build()?;

    let table = sqlx::query_as_with::<Postgres, AppTable, PgArguments>(&sql, args)
        .fetch_one(&mut transaction)
        .await
        .map_err(map_sqlx_error)?;

    let mut views = RepeatedView::default();
    if params.read_belongings {
        views.items = read_views_belong_to_id(&mut transaction, &table.id.to_string()).await?;
    }

    transaction
        .commit()
        .await
        .context("Failed to commit SQL transaction to read app.")?;

    let mut app: App = table.into();
    app.belongings = views;

    FlowyResponse::success().data(app)
}

pub(crate) async fn update_app(
    pool: &PgPool,
    params: UpdateAppParams,
) -> Result<FlowyResponse, ServerError> {
    let app_id = check_app_id(params.get_app_id().to_string())?;
    let name = match params.has_name() {
        false => None,
        true => Some(
            AppName::parse(params.get_name().to_owned())
                .map_err(invalid_params)?
                .0,
        ),
    };

    let workspace_id = match params.has_workspace_id() {
        false => None,
        true => Some(
            WorkspaceId::parse(params.get_workspace_id().to_owned())
                .map_err(invalid_params)?
                .0,
        ),
    };

    let color_style = match params.has_color_style() {
        false => None,
        true => {
            let color_bytes = params.get_color_style().write_to_bytes()?;
            Some(color_bytes)
        },
    };

    let desc = match params.has_desc() {
        false => None,
        true => Some(
            AppDesc::parse(params.get_desc().to_owned())
                .map_err(invalid_params)?
                .0,
        ),
    };

    let mut transaction = pool
        .begin()
        .await
        .context("Failed to acquire a Postgres connection to update app")?;

    let (sql, args) = SqlBuilder::update("app_table")
        .add_some_arg("name", name)
        .add_some_arg("workspace_id", workspace_id)
        .add_some_arg("color_style", color_style)
        .add_some_arg("description", desc)
        .add_some_arg("modified_time", Some(Utc::now()))
        .add_arg_if(params.has_is_trash(), "is_trash", params.get_is_trash())
        .and_where_eq("id", app_id)
        .build()?;

    sqlx::query_with(&sql, args)
        .execute(&mut transaction)
        .await
        .map_err(map_sqlx_error)?;

    transaction
        .commit()
        .await
        .context("Failed to commit SQL transaction to update app.")?;

    Ok(FlowyResponse::success())
}

pub(crate) async fn delete_app(pool: &PgPool, app_id: &str) -> Result<FlowyResponse, ServerError> {
    let app_id = check_app_id(app_id.to_owned())?;
    let mut transaction = pool
        .begin()
        .await
        .context("Failed to acquire a Postgres connection to delete app")?;

    let (sql, args) = SqlBuilder::delete("app_table")
        .and_where_eq("id", app_id)
        .build()?;

    let _ = sqlx::query_with(&sql, args)
        .execute(&mut transaction)
        .await
        .map_err(map_sqlx_error)?;

    transaction
        .commit()
        .await
        .context("Failed to commit SQL transaction to delete app.")?;

    Ok(FlowyResponse::success())
}

// transaction must be commit from caller
pub(crate) async fn read_apps_belong_to_workspace<'c>(
    transaction: &mut Transaction<'c, Postgres>,
    workspace_id: &str,
) -> Result<Vec<App>, ServerError> {
    let workspace_id = WorkspaceId::parse(workspace_id.to_owned()).map_err(invalid_params)?;
    let (sql, args) = SqlBuilder::select("app_table")
        .add_field("*")
        .and_where_eq("workspace_id", workspace_id.0)
        .build()?;

    let tables = sqlx::query_as_with::<Postgres, AppTable, PgArguments>(&sql, args)
        .fetch_all(transaction)
        .await
        .map_err(map_sqlx_error)?;

    let apps = tables
        .into_iter()
        .map(|table| table.into())
        .collect::<Vec<App>>();

    Ok(apps)
}

fn check_app_id(id: String) -> Result<Uuid, ServerError> {
    let app_id = AppId::parse(id).map_err(invalid_params)?;
    let app_id = Uuid::parse_str(app_id.as_ref())?;
    Ok(app_id)
}