"""The campaigns: create, read, list, update and delete, through store/."""
import store


def create_campaign(path, name, budget_cents, channel):
    return store.open(path).campaigns.insert({"name": name, "budget_cents": budget_cents, "channel": channel})


def get_campaign(path, campaign_id):
    return store.open(path).campaigns.get(campaign_id)


def list_campaigns(path):
    return store.open(path).campaigns.all()


def find_campaigns(path, name):
    return store.open(path).campaigns.where("name", name)


def update_campaign(path, campaign_id, name, budget_cents, channel):
    return store.open(path).campaigns.update(campaign_id, {"name": name, "budget_cents": budget_cents, "channel": channel})


def delete_campaign(path, campaign_id):
    return store.open(path).campaigns.delete(campaign_id)
