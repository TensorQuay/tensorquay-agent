use super::Fixture;
use serde_json::json;
use tqa_gateway::User;

impl Fixture {
    pub async fn admin(&self) -> Result<(User, String), Box<dyn std::error::Error>> {
        let response = self.post("/api/v1/auth/bootstrap", &json!({"email":"admin@example.invalid", "password":self.password(), "token":self.bootstrap()})).await?;
        assert_eq!(response.status(), 201);
        self.login("admin@example.invalid").await
    }

    pub async fn member(&self, email: &str) -> Result<(User, String), Box<dyn std::error::Error>> {
        let hash =
            sqlx::query_scalar!("SELECT password_hash FROM users WHERE role='admin' LIMIT 1")
                .fetch_one(&self.pool)
                .await?;
        sqlx::query!("INSERT INTO users (id,email,password_hash,role) VALUES ($1,$2::text::citext,$3,'member')", uuid::Uuid::now_v7(), email, hash).execute(&self.pool).await?;
        self.login(email).await
    }

    pub async fn login(&self, email: &str) -> Result<(User, String), Box<dyn std::error::Error>> {
        let response = self
            .post(
                "/api/v1/auth/login",
                &json!({"email":email, "password":self.password()}),
            )
            .await?;
        assert_eq!(response.status(), 200);
        let cookie = response
            .headers()
            .get("set-cookie")
            .ok_or("missing session")?
            .to_str()?
            .split(';')
            .next()
            .ok_or("missing cookie")?
            .to_owned();
        Ok((response.json().await?, cookie))
    }
}
