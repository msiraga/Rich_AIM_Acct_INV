//! Database User Module
//!
//! Handles user storage and retrieval in the database.

use async_trait::async_trait;
use std::sync::Arc;
use tokio::sync::Mutex;
use uuid::Uuid;
use chrono::{DateTime, Utc};
use crate::database::models::{User, UserRole};
use crate::database::error::{DatabaseError, DatabaseResult};

/// User repository
#[async_trait]
pub trait UserRepository: Send + Sync {
    /// Create a new user
    async fn create(&self, user: User) -> DatabaseResult<User>;
    
    /// Find a user by ID
    async fn find_by_id(&self, id: Uuid) -> DatabaseResult<Option<User>>;
    
    /// Find a user by username
    async fn find_by_username(&self, username: &str) -> DatabaseResult<Option<User>>;
    
    /// Find a user by email
    async fn find_by_email(&self, email: &str) -> DatabaseResult<Option<User>>;
    
    /// Find users by role
    async fn find_by_role(&self, role: UserRole) -> DatabaseResult<Vec<User>>;
    
    /// List all users
    async fn list_all(&self) -> DatabaseResult<Vec<User>>;
    
    /// Update a user
    async fn update(&self, id: Uuid, user: User) -> DatabaseResult<User>;
    
    /// Delete a user
    async fn delete(&self, id: Uuid) -> DatabaseResult<bool>;
    
    /// Check if a username exists
    async fn username_exists(&self, username: &str) -> DatabaseResult<bool>;
    
    /// Check if an email exists
    async fn email_exists(&self, email: &str) -> DatabaseResult<bool>;
    
    /// Update user password
    async fn update_password(&self, id: Uuid, password_hash: &str) -> DatabaseResult<()>;
    
    /// Update user last login
    async fn update_last_login(&self, id: Uuid) -> DatabaseResult<()>;
}

/// SurrealDB implementation of UserRepository
#[derive(Debug, Clone)]
pub struct SurrealUserRepository {
    /// Database connection
    pub db: Arc<Mutex<Option<surrealdb::Surreal<surrealdb::engine::local::Db>>>>,
}

impl SurrealUserRepository {
    /// Create a new SurrealDB user repository
    pub fn new(db: Arc<Mutex<Option<surrealdb::Surreal<surrealdb::engine::local::Db>>>>) -> Self {
        Self { db }
    }
}

#[async_trait]
impl UserRepository for SurrealUserRepository {
    async fn create(&self, user: User) -> DatabaseResult<User> {
        let guard = self.db.lock().await;
        let client = guard.as_ref().ok_or(DatabaseError::NotInitialized)?;

        let role_str = self.role_to_string(&user.role);
        let last_login_str = user.last_login.map(|dt| dt.to_rfc3339());

        let mut response = client.query(
            "CREATE user SET \
             username = $username, \
             email = $email, \
             password_hash = $password_hash, \
             display_name = $display_name, \
             role = $role, \
             is_active = $is_active, \
             last_login = $last_login, \
             created_at = $created_at, \
             updated_at = $updated_at"
        )
        .bind(("username", user.username.clone()))
        .bind(("email", user.email.clone()))
        .bind(("password_hash", user.password_hash.clone()))
        .bind(("display_name", user.display_name.clone()))
        .bind(("role", role_str))
        .bind(("is_active", user.is_active))
        .bind(("last_login", last_login_str))
        .bind(("created_at", user.created_at.to_rfc3339()))
        .bind(("updated_at", user.updated_at.to_rfc3339()))
        .await
        .map_err(|e| DatabaseError::QueryError(e.to_string()))?;

        let results: Vec<serde_json::Value> = response.take(0)
            .map_err(|e| DatabaseError::QueryError(e.to_string()))?;

        if let Some(val) = results.into_iter().next() {
            self.parse_user(val)
        } else {
            Ok(user)
        }
    }

    async fn find_by_id(&self, id: Uuid) -> DatabaseResult<Option<User>> {
        let guard = self.db.lock().await;
        let client = guard.as_ref().ok_or(DatabaseError::NotInitialized)?;

        let mut response = client.query(
            "SELECT * FROM user WHERE id = $id"
        )
        .bind(("id", id.to_string()))
        .await
        .map_err(|e| DatabaseError::QueryError(e.to_string()))?;

        let results: Vec<serde_json::Value> = response.take(0)
            .map_err(|e| DatabaseError::QueryError(e.to_string()))?;

        match results.into_iter().next() {
            Some(val) => Ok(Some(self.parse_user(val)?)),
            None => Ok(None),
        }
    }

    async fn find_by_username(&self, username: &str) -> DatabaseResult<Option<User>> {
        let guard = self.db.lock().await;
        let client = guard.as_ref().ok_or(DatabaseError::NotInitialized)?;

        let mut response = client.query(
            "SELECT * FROM user WHERE username = $username"
        )
        .bind(("username", username.to_string()))
        .await
        .map_err(|e| DatabaseError::QueryError(e.to_string()))?;

        let results: Vec<serde_json::Value> = response.take(0)
            .map_err(|e| DatabaseError::QueryError(e.to_string()))?;

        match results.into_iter().next() {
            Some(val) => Ok(Some(self.parse_user(val)?)),
            None => Ok(None),
        }
    }

    async fn find_by_email(&self, email: &str) -> DatabaseResult<Option<User>> {
        let guard = self.db.lock().await;
        let client = guard.as_ref().ok_or(DatabaseError::NotInitialized)?;

        let mut response = client.query(
            "SELECT * FROM user WHERE email = $email"
        )
        .bind(("email", email.to_string()))
        .await
        .map_err(|e| DatabaseError::QueryError(e.to_string()))?;

        let results: Vec<serde_json::Value> = response.take(0)
            .map_err(|e| DatabaseError::QueryError(e.to_string()))?;

        match results.into_iter().next() {
            Some(val) => Ok(Some(self.parse_user(val)?)),
            None => Ok(None),
        }
    }

    async fn find_by_role(&self, role: UserRole) -> DatabaseResult<Vec<User>> {
        let guard = self.db.lock().await;
        let client = guard.as_ref().ok_or(DatabaseError::NotInitialized)?;

        let role_str = self.role_to_string(&role);

        let mut response = client.query(
            "SELECT * FROM user WHERE role = $role"
        )
        .bind(("role", role_str))
        .await
        .map_err(|e| DatabaseError::QueryError(e.to_string()))?;

        let results: Vec<serde_json::Value> = response.take(0)
            .map_err(|e| DatabaseError::QueryError(e.to_string()))?;

        results.into_iter()
            .map(|val| self.parse_user(val))
            .collect()
    }

    async fn list_all(&self) -> DatabaseResult<Vec<User>> {
        let guard = self.db.lock().await;
        let client = guard.as_ref().ok_or(DatabaseError::NotInitialized)?;

        let mut response = client.query(
            "SELECT * FROM user"
        )
        .await
        .map_err(|e| DatabaseError::QueryError(e.to_string()))?;

        let results: Vec<serde_json::Value> = response.take(0)
            .map_err(|e| DatabaseError::QueryError(e.to_string()))?;

        results.into_iter()
            .map(|val| self.parse_user(val))
            .collect()
    }

    async fn update(&self, id: Uuid, user: User) -> DatabaseResult<User> {
        let guard = self.db.lock().await;
        let client = guard.as_ref().ok_or(DatabaseError::NotInitialized)?;

        // Verify the user exists
        let mut check = client.query(
            "SELECT * FROM user WHERE id = $id"
        )
        .bind(("id", id.to_string()))
        .await
        .map_err(|e| DatabaseError::QueryError(e.to_string()))?;

        let existing: Vec<serde_json::Value> = check.take(0)
            .map_err(|e| DatabaseError::QueryError(e.to_string()))?;

        if existing.is_empty() {
            return Err(DatabaseError::NotFound(format!("User with id {} not found", id)));
        }

        let role_str = self.role_to_string(&user.role);
        let last_login_str = user.last_login.map(|dt| dt.to_rfc3339());

        let mut response = client.query(
            "UPDATE user SET \
             username = $username, \
             email = $email, \
             password_hash = $password_hash, \
             display_name = $display_name, \
             role = $role, \
             is_active = $is_active, \
             last_login = $last_login, \
             updated_at = $updated_at \
             WHERE id = $id"
        )
        .bind(("username", user.username.clone()))
        .bind(("email", user.email.clone()))
        .bind(("password_hash", user.password_hash.clone()))
        .bind(("display_name", user.display_name.clone()))
        .bind(("role", role_str))
        .bind(("is_active", user.is_active))
        .bind(("last_login", last_login_str))
        .bind(("updated_at", user.updated_at.to_rfc3339()))
        .bind(("id", id.to_string()))
        .await
        .map_err(|e| DatabaseError::QueryError(e.to_string()))?;

        let results: Vec<serde_json::Value> = response.take(0)
            .map_err(|e| DatabaseError::QueryError(e.to_string()))?;

        if let Some(val) = results.into_iter().next() {
            self.parse_user(val)
        } else {
            Ok(user)
        }
    }

    async fn delete(&self, id: Uuid) -> DatabaseResult<bool> {
        let guard = self.db.lock().await;
        let client = guard.as_ref().ok_or(DatabaseError::NotInitialized)?;

        // Check if the user exists
        let mut check = client.query(
            "SELECT * FROM user WHERE id = $id"
        )
        .bind(("id", id.to_string()))
        .await
        .map_err(|e| DatabaseError::QueryError(e.to_string()))?;

        let existing: Vec<serde_json::Value> = check.take(0)
            .map_err(|e| DatabaseError::QueryError(e.to_string()))?;

        if existing.is_empty() {
            return Ok(false);
        }

        client.query(
            "DELETE FROM user WHERE id = $id"
        )
        .bind(("id", id.to_string()))
        .await
        .map_err(|e| DatabaseError::QueryError(e.to_string()))?;

        Ok(true)
    }

    async fn username_exists(&self, username: &str) -> DatabaseResult<bool> {
        let guard = self.db.lock().await;
        let client = guard.as_ref().ok_or(DatabaseError::NotInitialized)?;

        let mut response = client.query(
            "SELECT count() FROM user WHERE username = $username GROUP ALL"
        )
        .bind(("username", username.to_string()))
        .await
        .map_err(|e| DatabaseError::QueryError(e.to_string()))?;

        let results: Vec<serde_json::Value> = response.take(0)
            .map_err(|e| DatabaseError::QueryError(e.to_string()))?;

        let count = results.first()
            .and_then(|v| v.get("count"))
            .and_then(|c| c.as_u64())
            .unwrap_or(0);

        Ok(count > 0)
    }

    async fn email_exists(&self, email: &str) -> DatabaseResult<bool> {
        let guard = self.db.lock().await;
        let client = guard.as_ref().ok_or(DatabaseError::NotInitialized)?;

        let mut response = client.query(
            "SELECT count() FROM user WHERE email = $email GROUP ALL"
        )
        .bind(("email", email.to_string()))
        .await
        .map_err(|e| DatabaseError::QueryError(e.to_string()))?;

        let results: Vec<serde_json::Value> = response.take(0)
            .map_err(|e| DatabaseError::QueryError(e.to_string()))?;

        let count = results.first()
            .and_then(|v| v.get("count"))
            .and_then(|c| c.as_u64())
            .unwrap_or(0);

        Ok(count > 0)
    }

    async fn update_password(&self, id: Uuid, password_hash: &str) -> DatabaseResult<()> {
        let guard = self.db.lock().await;
        let client = guard.as_ref().ok_or(DatabaseError::NotInitialized)?;

        // Verify the user exists
        let mut check = client.query(
            "SELECT * FROM user WHERE id = $id"
        )
        .bind(("id", id.to_string()))
        .await
        .map_err(|e| DatabaseError::QueryError(e.to_string()))?;

        let existing: Vec<serde_json::Value> = check.take(0)
            .map_err(|e| DatabaseError::QueryError(e.to_string()))?;

        if existing.is_empty() {
            return Err(DatabaseError::NotFound(format!("User with id {} not found", id)));
        }

        client.query(
            "UPDATE user SET password_hash = $password_hash, updated_at = $updated_at WHERE id = $id"
        )
        .bind(("password_hash", password_hash.to_string()))
        .bind(("updated_at", Utc::now().to_rfc3339()))
        .bind(("id", id.to_string()))
        .await
        .map_err(|e| DatabaseError::QueryError(e.to_string()))?;

        Ok(())
    }

    async fn update_last_login(&self, id: Uuid) -> DatabaseResult<()> {
        let guard = self.db.lock().await;
        let client = guard.as_ref().ok_or(DatabaseError::NotInitialized)?;

        // Verify the user exists
        let mut check = client.query(
            "SELECT * FROM user WHERE id = $id"
        )
        .bind(("id", id.to_string()))
        .await
        .map_err(|e| DatabaseError::QueryError(e.to_string()))?;

        let existing: Vec<serde_json::Value> = check.take(0)
            .map_err(|e| DatabaseError::QueryError(e.to_string()))?;

        if existing.is_empty() {
            return Err(DatabaseError::NotFound(format!("User with id {} not found", id)));
        }

        let now = Utc::now().to_rfc3339();
        client.query(
            "UPDATE user SET last_login = $last_login WHERE id = $id"
        )
        .bind(("last_login", now))
        .bind(("id", id.to_string()))
        .await
        .map_err(|e| DatabaseError::QueryError(e.to_string()))?;

        Ok(())
    }
}

impl SurrealUserRepository {
    /// Convert UserRole to string
    fn role_to_string(&self, role: &UserRole) -> String {
        match role {
            UserRole::Admin => "admin",
            UserRole::Manager => "manager",
            UserRole::User => "user",
            UserRole::Viewer => "viewer",
            UserRole::Guest => "guest",
        }.to_string()
    }

    /// Parse a SurrealDB result into a User
    fn parse_user(&self, value: serde_json::Value) -> DatabaseResult<User> {
        let obj = value.as_object().ok_or(DatabaseError::DeserializationError("Expected object".to_string()))?;

        let id = obj.get("id").and_then(|v| v.as_str()).and_then(|s| Uuid::parse_str(s).ok()).unwrap_or_else(Uuid::new_v4);
        let username = obj.get("username").and_then(|v| v.as_str()).unwrap_or("").to_string();
        let email = obj.get("email").and_then(|v| v.as_str()).unwrap_or("").to_string();
        let password_hash = obj.get("password_hash").and_then(|v| v.as_str()).unwrap_or("").to_string();
        let display_name = obj.get("display_name").and_then(|v| v.as_str()).unwrap_or("").to_string();
        let role_str = obj.get("role").and_then(|v| v.as_str()).unwrap_or("user");
        let role = match role_str {
            "admin" => UserRole::Admin,
            "manager" => UserRole::Manager,
            "user" => UserRole::User,
            "viewer" => UserRole::Viewer,
            "guest" => UserRole::Guest,
            _ => UserRole::User,
        };
        let is_active = obj.get("is_active").and_then(|v| v.as_bool()).unwrap_or(true);
        let last_login = obj.get("last_login").and_then(|v| v.as_str()).and_then(|s| DateTime::parse_from_rfc3339(s).ok()).map(|dt| dt.with_timezone(&Utc));
        let created_at = obj.get("created_at").and_then(|v| v.as_str()).and_then(|s| DateTime::parse_from_rfc3339(s).ok()).map(|dt| dt.with_timezone(&Utc)).unwrap_or_else(|| Utc::now());
        let updated_at = obj.get("updated_at").and_then(|v| v.as_str()).and_then(|s| DateTime::parse_from_rfc3339(s).ok()).map(|dt| dt.with_timezone(&Utc)).unwrap_or_else(|| Utc::now());

        Ok(User {
            id,
            username,
            email,
            password_hash,
            display_name,
            role,
            is_active,
            last_login,
            created_at,
            updated_at,
        })
    }
}

/// In-memory implementation of UserRepository for testing
#[derive(Debug, Clone, Default)]
pub struct MemoryUserRepository {
    /// In-memory storage
    pub users: Arc<Mutex<Vec<User>>>,
}

impl MemoryUserRepository {
    /// Create a new in-memory user repository
    pub fn new() -> Self {
        Self::default()
    }
}

#[async_trait]
impl UserRepository for MemoryUserRepository {
    async fn create(&self, user: User) -> DatabaseResult<User> {
        let mut users = self.users.lock().await;
        users.push(user.clone());
        Ok(user)
    }

    async fn find_by_id(&self, id: Uuid) -> DatabaseResult<Option<User>> {
        let users = self.users.lock().await;
        Ok(users.iter().find(|u| u.id == id).cloned())
    }

    async fn find_by_username(&self, username: &str) -> DatabaseResult<Option<User>> {
        let users = self.users.lock().await;
        Ok(users.iter().find(|u| u.username == username).cloned())
    }

    async fn find_by_email(&self, email: &str) -> DatabaseResult<Option<User>> {
        let users = self.users.lock().await;
        Ok(users.iter().find(|u| u.email == email).cloned())
    }

    async fn find_by_role(&self, role: UserRole) -> DatabaseResult<Vec<User>> {
        let users = self.users.lock().await;
        Ok(users.iter().filter(|u| u.role == role).cloned().collect())
    }

    async fn list_all(&self) -> DatabaseResult<Vec<User>> {
        let users = self.users.lock().await;
        Ok(users.clone())
    }

    async fn update(&self, id: Uuid, user: User) -> DatabaseResult<User> {
        let mut users = self.users.lock().await;
        
        if let Some(index) = users.iter().position(|u| u.id == id) {
            users[index] = user.clone();
            Ok(user)
        } else {
            Err(DatabaseError::NotFound(format!("User with id {} not found", id)))
        }
    }

    async fn delete(&self, id: Uuid) -> DatabaseResult<bool> {
        let mut users = self.users.lock().await;
        let len_before = users.len();
        users.retain(|u| u.id != id);
        Ok(len_before != users.len())
    }

    async fn username_exists(&self, username: &str) -> DatabaseResult<bool> {
        let users = self.users.lock().await;
        Ok(users.iter().any(|u| u.username == username))
    }

    async fn email_exists(&self, email: &str) -> DatabaseResult<bool> {
        let users = self.users.lock().await;
        Ok(users.iter().any(|u| u.email == email))
    }

    async fn update_password(&self, id: Uuid, password_hash: &str) -> DatabaseResult<()> {
        let mut users = self.users.lock().await;
        
        if let Some(user) = users.iter_mut().find(|u| u.id == id) {
            user.password_hash = password_hash.to_string();
            Ok(())
        } else {
            Err(DatabaseError::NotFound(format!("User with id {} not found", id)))
        }
    }

    async fn update_last_login(&self, id: Uuid) -> DatabaseResult<()> {
        let mut users = self.users.lock().await;
        
        if let Some(user) = users.iter_mut().find(|u| u.id == id) {
            user.last_login = Some(Utc::now());
            Ok(())
        } else {
            Err(DatabaseError::NotFound(format!("User with id {} not found", id)))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    #[tokio::test]
    async fn test_memory_user_repository() {
        let repo = MemoryUserRepository::new();
        
        let user = User::new("john_doe", "john@example.com", "John Doe");
        
        // Create user
        let created = repo.create(user.clone()).await.unwrap();
        assert_eq!(created.username, "john_doe");
        
        // Find by ID
        let found = repo.find_by_id(created.id).await.unwrap();
        assert!(found.is_some());
        assert_eq!(found.unwrap().username, "john_doe");
        
        // Find by username
        let found = repo.find_by_username("john_doe").await.unwrap();
        assert!(found.is_some());
        
        // Find by email
        let found = repo.find_by_email("john@example.com").await.unwrap();
        assert!(found.is_some());
        
        // Check username exists
        let exists = repo.username_exists("john_doe").await.unwrap();
        assert!(exists);
        
        // Check email exists
        let exists = repo.email_exists("john@example.com").await.unwrap();
        assert!(exists);
        
        // List all
        let all = repo.list_all().await.unwrap();
        assert_eq!(all.len(), 1);
        
        // Delete
        let deleted = repo.delete(created.id).await.unwrap();
        assert!(deleted);
        
        let all = repo.list_all().await.unwrap();
        assert_eq!(all.len(), 0);
    }

    #[tokio::test]
    async fn test_user_operations() {
        let repo = MemoryUserRepository::new();
        
        // Create a user
        let mut user = User::new("jane_doe", "jane@example.com", "Jane Doe");
        user = repo.create(user).await.unwrap();
        
        // Update the user
        let mut updated_user = user.clone();
        updated_user.display_name = "Jane Smith".to_string();
        let updated = repo.update(user.id, updated_user).await.unwrap();
        assert_eq!(updated.display_name, "Jane Smith");
        
        // Update password
        repo.update_password(user.id, "new_password_hash").await.unwrap();
        let found = repo.find_by_id(user.id).await.unwrap();
        assert_eq!(found.unwrap().password_hash, "new_password_hash");
        
        // Update last login
        repo.update_last_login(user.id).await.unwrap();
        let found = repo.find_by_id(user.id).await.unwrap();
        assert!(found.unwrap().last_login.is_some());
    }

    #[tokio::test]
    async fn test_find_by_role() {
        let repo = MemoryUserRepository::new();
        
        // Create users with different roles
        let mut admin = User::new("admin", "admin@example.com", "Admin");
        admin.role = UserRole::Admin;
        repo.create(admin).await.unwrap();
        
        let mut user = User::new("user", "user@example.com", "User");
        user.role = UserRole::User;
        repo.create(user).await.unwrap();
        
        // Find by role
        let admins = repo.find_by_role(UserRole::Admin).await.unwrap();
        assert_eq!(admins.len(), 1);
        
        let users = repo.find_by_role(UserRole::User).await.unwrap();
        assert_eq!(users.len(), 1);
    }
}
