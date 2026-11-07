# Keys for web services

<!-- Tests: WEB-U-139, WEB-U-140, WEB-U-141, WEB-U-142, WEB-U-143. -->
A key for a web service lets your tasks call that service as you: an API token for GitHub, a package registry's
password, a service's API key. The agent never holds the key itself. Each run gets a stand-in for it, and the gateway
swaps in the real key only on the way to the key's web address, only for that run, and only in the place the service
expects it. Anywhere else the stand-in is worthless, so a script that prints or sends it elsewhere gives nothing away.

Your keys are yours alone. Nobody can read a key back once it is saved, you included; administrators can see that a
key of yours exists (its web address and when it was last used) and delete it, but never its name or its value.

Open **Keys** (in your account's settings). **Mine** lists two kinds of key, each under its own heading:
**Keys for web services**, for a service your tasks call over the web, such as GitHub or a package registry; and
**Keys for connectors**, for a connector your administrator set up that works as you, such as a team's issue tracker.
**Add a key for a connector** asks which connector the key is for; when none of the connectors needs a key of yours,
the section says so and offers nothing to add. **Shared** lists the shared keys an administrator lets you use (see [Shared keys](#shared-keys)).

## Add a key
Choose **Add a key for a web service**. **Add a key** opens as a page with three parts; each problem is named at its
field before anything is sent, and leaving with unsaved changes asks first.

**Where it's used**
- **Web address**: the service's address, such as `api.github.com`. Add a path to limit the key to part of the
  service, such as one organisation: `github.com/acme`. It is one exact host (no `*`), over https only, without a
  name, password, or anything after `?` or `#`; a port only when it isn't 443 (`git.acme.lan:8443`). It may be an
  exact address instead of a name: `10.0.0.5`, or an IPv6 address in brackets, `[fd00::5]:8443`.
- **Name**: what your tasks call the key. They read it as `$GITHUB_WORK`, and the agent is told the key's name and web
  address, never its value. See [Names](#names).

**How the service expects it**
- **As a token**: most APIs. It is sent as `Authorization: Bearer …`.
- **With a user name**: code hosting and package registries. Enter the **User name** the service expects with this
  key.

**The key**
- **Key**: paste it once, on one line (up to 8 kB). "Paste it once. Nobody can read it back, you included."

Then **Save**. The key is listed under **Keys for web services**. When a task calls the service, the agent uses the
key's name in its commands, for example `curl -H "Authorization: Bearer $GITHUB_WORK" https://api.github.com/user`,
and Git over https uses a key for a Git host by itself.

A key works only while tasks may reach its web address. When they can't, its row says so under it: "Tasks can't
reach … yet. An administrator can allow it." An administrator gets a link to allow it in **Internet access** instead.
A key for an address needs a rule for exactly that address: "Tasks can't reach … yet. An administrator can allow that
exact address."

### Names
A key's name is made of capital letters, digits and `_`, starts with a letter, and is up to 64 characters long.

- The page suggests one from the web address while you haven't typed your own: the usual name for well-known services
  (`GITHUB_TOKEN` for github.com, `GITLAB_TOKEN`, `HF_TOKEN` for Hugging Face, `NPM_TOKEN`, `PYPI_TOKEN`), otherwise
  the address's first part in capitals with `_TOKEN` (an IP address whole: `HOST_10_0_0_5_TOKEN`). A name you already
  use gets `_2`, `_3`.
- Some names are kept for the task's own use: the names a task's commands rely on, such as `PATH`, `HOME` or
  `HTTPS_PROXY`, and any name starting `LC_`, `LD_`, `GIT_` or `TQA_`.
- Names are unique across your own keys and the shared keys you may use, so a name always means one key. For the same
  web address, your own key is used before a shared one.

Keys that used to save a task's changes to a project were moved here as ordinary keys, named from their address and
marked **Check the name**: "This key's name was chosen for you from its address. Check it, then Save." The key works
meanwhile; **Save** on its page, with the name you want, clears the mark.

### Refused before saving
| When | The page says |
|---|---|
| The address isn't https | "Keys are sent only over https." |
| The address has a name, password, `?` or `#` | "Use the service's address only, without a name, password, or anything after ? or #." |
| The address has a `*` | "One exact address; add another key for another host." |
| The address is an IPv6 address without brackets | "Put an IPv6 address in brackets, such as [fd00::5]." |
| The address isn't one | "That isn't a web address, such as github.com/acme or 10.0.0.5." |
| The name isn't valid | "Use capital letters, digits and _, starting with a letter." |
| The name is kept for the task | "… is kept for the task's own use. Pick another name." |
| The name is another key's | "… is already your key for …. Pick another name." or "… is already the shared key for …. Pick another name." |
| A user name is missing | "Enter the user name the service expects with this key." |
| The key is missing, on two lines or too long | "Paste the key." or "Paste the key on one line, up to 8 kB." |
| You have 64 keys | "You have as many keys as you can keep (64). Delete one first." |
| The key store isn't available | "The key could not be saved. Try again in a moment." |

## A key's page
<!-- Tests: WEB-U-143. -->
Choose a key's name in the list to open its page: its web address and how it is sent, **Last used**, its fields, and
**The key**.

- **Last used** says when and in which task, such as "Today, 14:02 · Sales report", or **Never used**; a last try that
  was refused says why. **Show all uses** lists every use and refusal, newest first.
- **The key** shows only when it was set. **Replace…** turns that line into an empty **New key** field;
  **Keep the current key** turns it back. Save to replace it.
- Change the web address, name or user name and **Save**. If someone changed the key while you had it open: "Someone
  changed this key since you opened it. Your edits are kept: save again to replace their change with yours."
- **Delete…**, in the ⋯ menu, asks first and says what stops working: "Tasks can't use … with it any more. A run
  that's using it now loses it at once." Then **Delete key**.

## Shared keys
<!-- Tests: WEB-U-146. -->
**Shared** lists **Shared keys you may use**: keys an administrator added for everyone or for you, with their name,
web address and when you last used them. You can't change them, and you never see anyone else's use of them. With
none: "No shared keys for you yet."

Your tasks get these keys as they get your own, by name. How administrators add them is in the
[administrator's guide](../admin-guide/web-settings.md#shared-keys-for-web-services).

## What a task shows
<!-- Tests: WEB-U-147, WEB-U-148. -->
A run gets the keys of the person whose message started it: yours when you sent it, your own keys and the shared keys
you may use, for the web addresses the task may reach. In a channel or on a shared workspace, a run someone else
started gets their keys, never yours.

When a command calls a service with a key, the step that ran it says so under its command: "Called … with …", such as
Called api.github.com with GITHUB_WORK. Only the person whose run it is sees the key's name; anyone else who can see
the task reads "Called … with a key".

When the gateway doesn't send a key, the step says why, and nothing reached the service:

| Why | The step says |
|---|---|
| A process left over from an earlier run used the key | "… wasn't sent to …: it came from an earlier run, and each run has its own." |
| The key came from another task | "… wasn't sent to …: it came from another task." |
| The run had ended | "… wasn't sent to …: the run had already ended." |
| It was sent to another address than the key's | "… wasn't sent to …: it's a key for another address." |
| It was put somewhere else in the request than the service expects | "… wasn't sent to …: it was put somewhere other than where this key goes." |
| Tasks may no longer reach the address | "… wasn't sent to …: tasks can't reach this address any more." |
| The key was deleted during the run | "… wasn't sent to …: the key was deleted while the run was using it." |
| Its use couldn't be recorded | "… wasn't sent to …: its use couldn't be recorded. Try again in a moment." |
| The task used a key that isn't one of yours | "The task used a key for … that isn't set up." |

For the last, the person whose run it is also gets "Add a key for …", which opens **Add a key** with the address
filled in; add the key, then ask the agent to try again. Anyone else reads the same lines with "A key" in place of
the key's name, and no link. A refused try also shows in the key's **Last used**, in the same words.
