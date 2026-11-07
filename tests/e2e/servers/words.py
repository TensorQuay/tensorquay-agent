import json,sys
for l in sys.stdin:
 m=json.loads(l);i=m.get('id');t=m.get('method')
 if i is None:continue
 if t=='initialize':r={'protocolVersion':'2025-11-25','capabilities':{'tools':{}},'serverInfo':{'name':'words','version':'1'}}
 elif t=='tools/list':r={'tools':[{'name':'count_words','description':'Count the words in a text','inputSchema':{'type':'object','properties':{'text':{'type':'string'}},'required':['text']}}]}
 elif t=='tools/call':
  p=m.get('params') or {};a=p.get('arguments') or {}
  if p.get('name')=='count_words' and isinstance(a.get('text'),str):r={'content':[{'type':'text','text':'words: %d'%len(a['text'].split())}]}
  else:r={'content':[{'type':'text','text':'unknown tool or arguments'}],'isError':True}
 else:r={}
 print(json.dumps({'jsonrpc':'2.0','id':i,'result':r}),flush=True)
