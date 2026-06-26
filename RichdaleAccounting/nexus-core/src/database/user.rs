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
    pub db: Arc<Mutex<Option<surrealdb::Surreal<surrealdb::engine::remote::ws::Client>>>>,
}

impl SurrealUserRepository {
    /// Create a new SurrealDB user repository
    pub fn new(db: Arc<Mutex<Option<surrealdb::Surreal<surrealdb::engine::remote::ws::Client>>>>) -> Self {
        Self { db }
    }
}

#[async_trait]
impl UserRepository for SurrealUserRepository {
    async fn create(&self, user: User) -> DatabaseResult<User> {
        let client = self.db.lock().await;
        let client = client.as_ref().ok_or(DatabaseError::NotInitialized)?;

        let user_data = serde_json::json!({
            "id": user.id.to_string(),
            "username": user.username,
            "email": user.email,
            "password_hash": user.password_hash,
            "display_name": user.display_name,
            "role": self.role_to_string(&user.role),
            "is_active": user.is_active,
            "last_login": user.last_login.map(|d| d.to_rfc3339()),
            "created_at": user.created_at.to_rfc3339(),
            "updated_at": user.updated_at.to_rfc3339(),
        });

        let query = format!(
            "INSERT INTO user CONTENT {};",
            serde_json::to_string(&user_data).map_err(|e| DatabaseError::SerializationError(e.to_string()))?
        );

        let _ = client.query(&query)
            .await
            .map_err(|e| DatabaseError::QueryError(e.to_string()))?;

        Ok(user)
    }

    async fn find_by_id(&self, id: Uuid) -> DatabaseResult<Option<User>> {
        let client = self.db.lock().await;
        let client = client.as_ref().ok_or(DatabaseError::NotInitialized)?;

        let query = format!("SELECT * FROM user WHERE id = '{}';", id);
        
        let result: Option<serde_json::Value> = client.query(&query)
            .await
            .map_err(|e| DatabaseError::QueryError(e.to_string()))?;

        match result {
            Some(value) => {
                let user = self.parse_user(value)?;
                Ok(Some(user))
            }
            None => Ok(None),
        }
    }

    async fn find_by_username(&self, username: &str) -> DatabaseResult<Option<User>> {
        let client = self.db.lock().await;
        let client = client.as_ref().ok_or(DatabaseError::NotInitialized)?;

        let query = format!("SELECT * FROM user WHERE username = '{}';", username);
        
        let result: Option<serde_json::Value> = client.query(&query)
            .await
            .map_err(|e| DatabaseError::QueryError(e.to_string()))?;

        match result {
            Some(value) => {
                let user = self.parse_user(value)?;
                Ok(Some(user))
            }
            None => Ok(None),
        }
    }

    async fn find_by_email(&self, email: &str) -> DatabaseResult<Option<User>> {
        let client = self.db.lock().await;
        let client = client.as_ref().ok_or(DatabaseError::NotInitialized)?;

        let query = format!("SELECT * FROM user WHERE email = '{}';", email);
        
        let result: Option<serde_json::Value> = client.query(&query)
            .await
            .map_err(|e| DatabaseError::QueryError(e.to_string()))?;

        match result {
            Some(value) => {
                let user = self.parse_user(value)?;
                Ok(Some(user))
            }
            None => Ok(None),
        }
    }

    async fn find_by_role(&self, role: UserRole) -> DatabaseResult<Vec<User>> {
        let client = self.db.lock().await;
        let client = client.as_ref().ok_or(DatabaseError::NotInitialized)?;

        let role_str = self.role_to_string(&role);
        let query = format!("SELECT * FROM user WHERE role = '{}';", role_str);
        
        let result: Vec<serde_json::Value> = client.query(&query)
            .await
            .map_err(|e| DatabaseError::QueryError(e.to_string()))?;

        let users = result.into_iter()
            .filter_map(|v| self.parse_user(v).ok())
            .collect();

        Ok(users)
    }

    async fn list_all(&self) -> DatabaseResult<Vec<User>> {
        let client = self.db.lock().await;
        let client = client.as_ref().ok_or(DatabaseError::NotInitialized)?;

        let query = "SELECT * FROM user;";
        
        let result: Vec<serde_json::Value> = client.query(query)
            .await
            .map_err(|e| DatabaseError::QueryError(e.to_string()))?;

        let users = result.into_iter()
            .filter_map(|v| self.parse_user(v).ok())
            .collect();

        Ok(users)
    }

    async fn update(&self, id: Uuid, user: User) -> DatabaseResult<User> {
        let client = self.db.lock().await;
        let client = client.as_ref().ok_or(DatabaseError::NotInitialized)?;

        let user_data = serde_json::json!({
            "id": id.to_string(),
            "username": user.username,
            "email": user.email,
            "password_hash": user.password_hash,
            "display_name": user.display_name,
            "role": self.role_to_string(&user.role),
            "is_active": user.is_active,
            "last_login": user.last_login.map(|d| d.to_rfc3339()),
            "updated_at": user.updated_at.to_rfc3339(),
        });

        let query = format!(
            "UPDATE user SET {} WHERE id = '{}';",
            serde_json::to_string(&user_data).map_err(|e| DatabaseError::SerializationError(e.to_string()))?,
            id
        );

        let _ = client.query(&query)
            .await
            .map_err(|e| DatabaseError::QueryError(e.to_string()))?;

        Ok(user)
    }

    async fn delete(&self, id: Uuid) -> DatabaseResult<bool> {
        let client = self.db.lock().await;
        let client = client.as_ref().ok_or(DatabaseError::NotInitialized)?;

        let query = format!("DELETE FROM user WHERE id = '{}';", id);
        
        let result: Option<serde_json::Value> = client.query(&query)
            .await
            .map_err(|e| DatabaseError::QueryError(e.to_string()))?;

        Ok(result.is_some())
    }

    async fn username_exists(&self, username: &str) -> DatabaseResult<bool> {
        let user = self.find_by_username(username).await?;
        Ok(user.is_some())
    }

    async fn email_exists(&self, email: &str) -> DatabaseResult<bool> {
        let user = self.find_by_email(email).await?;
        Ok(user.is_some())
    }

    async fn update_password(&self, id: Uuid, password_hash: &str) -> DatabaseResult<()> {
        let client = self.db.lock().await;
        let client = client.as_ref().ok_or(DatabaseError::NotInitialized)?;

        let query = format!(
            "UPDATE user SET password_hash = '{}' WHERE id = '{}';",
            password_hash, id
        );

        let _ = client.query(&query)
            .await
            .map_err(|e| DatabaseError::QueryError(e.to_string()))?;

        Ok(())
    }

    async fn update_last_login(&self, id: Uuid) -> DatabaseResult<()> {
        let client = self.db.lock().await;
        let client = client.as_ref().ok_or(DatabaseError::NotInitialized)?;

        let now = Utc::now().to_rfc3339();
        let query = format!(
            "UPDATE user SET last_login = '{}' WHERE id = '{}';",
            now, id
        );

        let _ = client.query(&query)
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
        let last_login = obj.get("last_login").and_then(|v| v.as_str()).and_then(|s| DateTime::parse_from_rfc3339(s).ok());
        let created_at = obj.get("created_at").and_then(|v| v.as_str()).and_then(|s| DateTime::parse_from_rfc3339(s).ok()).unwrap_or_else(|| Utc::now());
        let updated_at = obj.get("updated_at").and_then(|v| v.as_str()).and_then(|s| DateTime::parse_from_rfc3339(s).ok()).unwrap_or_else(|| Utc::now());

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
