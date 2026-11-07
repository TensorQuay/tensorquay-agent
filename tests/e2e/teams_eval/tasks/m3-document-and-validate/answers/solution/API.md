# Items API

## GET /health

200 `{"status": "ok"}`

## GET /items

query `limit` (default 50); 200 `{"items": [...]}`

## POST /items

body `{name, price}`; 201 the item; 400 when invalid

## GET /items/<id>

200 the item; 404

## PUT /items/<id>

body with `name` and/or `price`; 200; 400; 404

## DELETE /items/<id>

204; 404
