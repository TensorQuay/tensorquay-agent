"""The teams: create, read, list, update and delete, through store/."""
import store


def create_team(path, name, lead_id, size):
    return store.open(path).teams.insert({"name": name, "lead_id": lead_id, "size": size})


def get_team(path, team_id):
    return store.open(path).teams.get(team_id)


def list_teams(path):
    return store.open(path).teams.all()


def find_teams(path, name):
    return store.open(path).teams.where("name", name)


def update_team(path, team_id, name, lead_id, size):
    return store.open(path).teams.update(team_id, {"name": name, "lead_id": lead_id, "size": size})


def delete_team(path, team_id):
    return store.open(path).teams.delete(team_id)
