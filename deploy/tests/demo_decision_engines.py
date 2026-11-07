"""WEB-E2E-070 (D11's positive path): the settings suite's two decision engines, served by its own fake model
(`decision_engines` in crates/tqa-testkit/scenarios/demo.json, TK-U-048). Plain http needs `insecure_http`, which only
the configuration file may set, so they're declared in gateway.toml, never added in Settings."""

import compose

ENGINES = (
    ("stopper", "Test engine that stops"),
    ("passer", "Test engine that passes"),
)


def engines_toml():
    """The two engines' entries, each at the fake model's `/engines/<name>`."""
    return "".join(
        "\n[[decision_engines]]\nname = \"{0}\"\ndisplay_name = \"{1}\"\n"
        "url = \"http://fake-model:8080/engines/{0}\"\ninsecure_http = true\n".format(name, display)
        for name, display in ENGINES)


def configure(directory, env):
    """Appends the engines to the stack's gateway.toml (arrays of tables last, so nothing after them is captured)."""
    compose.ownership(directory, env)
    config = directory / "gateway.toml"
    compose.write(config, config.read_text() + engines_toml())
    compose.ownership(directory, env, "1000:1000")
