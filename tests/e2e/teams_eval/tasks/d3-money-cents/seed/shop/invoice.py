from shop.cart import Cart


def lines(cart: Cart):
    return [f"{quantity} x {price} = {price * quantity}" for price, quantity, _ in cart.lines]


def summary(cart: Cart):
    return {"net": str(cart.net()), "tax": str(cart.tax()), "gross": str(cart.gross())}
