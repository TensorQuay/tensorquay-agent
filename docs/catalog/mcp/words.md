# words

Counts the words in a text. A minimal stdio MCP server in this repository, used by E2E-MCP-006 and as the catalogue's
first entry: it needs nothing but the Workstation image's own `python3`.

| Field | Value |
|---|---|
| Name | words |
| Kind | mcp |
| Source | `tests/e2e/servers/words.py` in this repository |
| Version | this repository at f2c51d2 |
| Transport | stdio |
| Tested | ext-check f2c51d287bfd, 2026-09-29T06:12:59.05823056Z |
| Report | words.ext-check.json |
| Tools SHA-256 | 7d22bb2a15c7b5c5f62687e1e9f1b2d7203745357484656f4b7265dabaab809c |

## Configuration

The whole server is one argument (at most 1 KiB), so no image change is needed.

```toml
[[mcp.servers]]
name = "words"
command = "/usr/bin/python3"
args = ["-c", "import json,sys\nfor l in sys.stdin:\n m=json.loads(l);i=m.get('id');t=m.get('method')\n if i is None:continue\n if t=='initialize':r={'protocolVersion':'2025-11-25','capabilities':{'tools':{}},'serverInfo':{'name':'words','version':'1'}}\n elif t=='tools/list':r={'tools':[{'name':'count_words','description':'Count the words in a text','inputSchema':{'type':'object','properties':{'text':{'type':'string'}},'required':['text']}}]}\n elif t=='tools/call':\n  p=m.get('params') or {};a=p.get('arguments') or {}\n  if p.get('name')=='count_words' and isinstance(a.get('text'),str):r={'content':[{'type':'text','text':'words: %d'%len(a['text'].split())}]}\n  else:r={'content':[{'type':'text','text':'unknown tool or arguments'}],'isError':True}\n else:r={}\n print(json.dumps({'jsonrpc':'2.0','id':i,'result':r}),flush=True)\n"]
```

## Tools

| Tool | Model name |
|---|---|
| count_words | mcp__words__count_words |

## Notes

- Tested in the Workstation template image (`sha256:cb399c4ac4724c1449e891d31d7aa979f48adedad8d652405b0033ddfc8abeda`), without network, with one known-answer call
  (`count_words` on four words answers `words: 4`).
- It answers a tool it doesn't list, or arguments without `text`, with an error result.
