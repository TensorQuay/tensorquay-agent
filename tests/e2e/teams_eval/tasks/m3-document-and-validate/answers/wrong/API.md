# Items API

## GET /health

200 `{"status": "ok"}`

## GET /items

query `limit` (default 50); 200 `{"items": [...]}`

## POST /items

body `{name, price}`; 201 the item; 400 when invalid
