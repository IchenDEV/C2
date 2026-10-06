
import json,sys,hashlib,os,socket,uuid,time
servers=[]
cwd=""
def tool(op):
    server=next(s for s in servers if s['name']=='codetwo_coordination'); env={e['name']:e['value'] for e in server['env']}
    host,port=env['CODETWO_COORDINATION_ADDRESS'].rsplit(':',1)
    with socket.create_connection((host,int(port)),timeout=10) as sock:
        sock.sendall((json.dumps({'session':env['CODETWO_COORDINATION_SESSION'],'key':env['CODETWO_COORDINATION_KEY'],'command_id':str(uuid.uuid4()),'operation':op})+'\n').encode())
        response=json.loads(sock.makefile().readline())
        if 'error' in response: raise RuntimeError(response['error'])
        return response['result']
for line in sys.stdin:
    m=json.loads(line); method=m.get('method'); mid=m.get('id')
    if method=='initialize': result={'protocolVersion':1}
    elif method in ('session/new','session/load'):
        servers=m['params'].get('mcpServers',[]); cwd=m['params'].get('cwd',os.getcwd())
        result={'sessionId':'fixture','models':{'currentModelId':'fixture-default','availableModels':[{'modelId':'fixture-default','name':'Fixture'}]}}
    elif method=='session/prompt':
        text='\n'.join(p.get('text','') for p in m['params']['prompt'])
        if 'INPUT DATA:\n' in text:
            data,_=json.JSONDecoder().raw_decode(text.split('INPUT DATA:\n')[-1].lstrip()); actions=[]
            if 'turn' in data:
                turn=data['turn']; content=turn['content']
                if '慢消息' in content: time.sleep(1.5)
                if '交办' in content:
                    actions=[{'kind':'route','project_path':p,'content':('Ask first report language' if '问题' in content else 'Inspect the isolated local report fixture and submit report.txt with matching hash.')} for p in turn['project_paths']]
                elif '记住' in content:
                    actions=[{'kind':'propose_memory','project_path':turn['project_paths'][0],'category':'preference','content':'项目简报使用中文。'}]
                elif '停止' in content and data.get('goals'):
                    actions=[{'kind':'control','goal_id':data['goals'][0]['id'],'operation':'stop','quote':'停止'}]
                output=json.dumps({'summary':'我会持续跟进这两个项目；讨论不会建立任务。' if not actions else '已记录你的交办或记忆建议，具体结果以原任务及回执为准。','actions':actions})
                print(json.dumps({'jsonrpc':'2.0','method':'session/update','params':{'sessionId':'fixture','update':{'sessionUpdate':'agent_message_chunk','content':{'type':'text','text':output}}}}),flush=True)
                print(json.dumps({'jsonrpc':'2.0','id':mid,'result':{'stopReason':'end_turn'}}),flush=True)
                continue
            for request in data.get('requests',[]):
                if not request['handled']:
                    actions.append({'kind':'create_goal','request_id':request['id'],'title':('Ask first manual report' if 'Ask first' in request['content'] else 'Requested report'),'acceptance':'Versioned report','priority':1})
            for g in data['goals']:
                if g['status']!='active': continue
                if not g['assignments']:
                    existing=next((s for s in data['sessions'] if s['project']==g['project_path'] and s.get('request','')=='Existing report'),None)
                    actions.append({'kind':'link','goal_id':g['id'],'session_id':existing['id']} if existing else {'kind':'dispatch','goal_id':g['id'],'instruction':'Inspect the local fixture report and provide its version and checks.'})
                else:
                    a=g['assignments'][-1]; s=next((s for s in data['sessions'] if s['id']==a['session_id']),None)
                    if s and s['activity']['state']['kind']=='idle' and s['output'] and (not a.get('protocol') or a.get('result')) and 'manual' not in g['title']:
                        actions.append({'kind':'accept','goal_id':g['id'],'session_id':s['id'],'activity_revision':s['activity']['revision'],'evidence':s['output'],'artifacts':a['result']['artifacts'] if a.get('result') else [{'path':'report.txt','sha256':hashlib.sha256(b'fixture report v1').hexdigest()}]})
            output=json.dumps({'summary':'Both project reports are tracked.', 'actions':actions})
        else:
            output='Fixture report v1; input checked. This is a transport fixture, not a real software verification.'
            if any(s['name']=='codetwo_coordination' for s in servers):
                context=tool({'operation':'context'}); version=context['goal']['contract_revision']
                tool({'operation':'confirm','contract_revision':version,'message_ids':[msg['id'] for msg in context['messages'] if msg['author']!='worker' and msg['contract_revision']==version and msg['state'] in ('queued','accepted')]})
                tool({'operation':'progress','content':'Fixture report inspected; ready to review.'})
                if 'Ask first' in context['goal']['title'] and version==1 and not context['questions']:
                    tool({'operation':'ask','title':'Which report language?','context':'Choose the output language before writing. Independent projects may continue.','options':['English','Chinese'],'blocking':True})
                elif 'Ask first' in context['goal']['title'] and version==1:
                    tool({'operation':'propose_change','title':'Chinese manual report','acceptance':'Produce report.txt with fixture report v2; provide its SHA-256 and actual content check.','reason':'The selected language changes the deliverable requirements.'})
                else:
                    data=b'fixture report v1' if version==1 else b'fixture report v2'
                    with open(os.path.join(cwd,'report.txt'),'wb') as f:f.write(data)
                    tool({'operation':'submit','contract_revision':version,'evidence':output,'artifacts':[{'path':'report.txt','sha256':hashlib.sha256(data).hexdigest()}]})
        print(json.dumps({'jsonrpc':'2.0','method':'session/update','params':{'sessionId':'fixture','update':{'sessionUpdate':'agent_message_chunk','content':{'type':'text','text':output}}}}),flush=True)
        result={'stopReason':'end_turn'}
    elif method=='session/set_model' and m['params'].get('modelId')=='missing-model':
        print(json.dumps({'jsonrpc':'2.0','id':mid,'error':{'code':-32602,'message':'Unsupported fixture model'}}),flush=True); continue
    elif method in ('session/set_model','session/set_config_option'): result={}
    else: continue
    print(json.dumps({'jsonrpc':'2.0','id':mid,'result':result}),flush=True)
