# Inventory API

Every function returns a plain dictionary or list and raises `ValueError` for invalid input.

### add_item(sku, quantity=1)

Add `quantity` units of `sku` to the inventory.

### remove_item(sku, quantity=1)

Remove units of `sku`; with `force`, allow the count to reach zero.
