# Vendor comparison

| Vendor | Auth | Rate limits | Pagination | Pricing |
|---|---|---|---|---|
| Arcwell | OAuth 2.0 client credentials | 600 requests per minute | cursor-based (`next_cursor`) | $0.002 per request |
| Brisa | an API key in the `X-Brisa-Key` header | 50 requests per second | offset and limit | $49 per month flat, unlimited requests |
| Corvane | HMAC-SHA256 signed requests | 10,000 requests per day | page numbers (`page` and `per_page`) | free up to 1,000 requests a month, then $0.01 per request |
