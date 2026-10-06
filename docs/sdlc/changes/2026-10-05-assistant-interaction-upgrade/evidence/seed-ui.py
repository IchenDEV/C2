from pathlib import Path
import sqlite3,json,time
root=Path('.codex/run/assistant-interaction-upgrade').resolve();project=root/'ui-project';project.mkdir(exist_ok=True)
c=sqlite3.connect(root/'ui-data/codetwo.db');now=int(time.time());path=str(project)
c.execute('INSERT INTO projects(path,name,last_opened_at) VALUES(?,?,?)',(path,'CodeTwo',now))
for sid,title in [('worker-a','A · retained execution'),('review-a','A · independent review'),('review-b','B · scoped failure')]:
 c.execute('INSERT INTO sessions(id,title,provider,cwd,project_path,permission_mode,created_at,activity_json) VALUES(?,?,?,?,?,?,?,?)',(sid,title,'grok',path,path,json.dumps('ask'),now,json.dumps({'revision':1,'state':{'kind':'idle'}})))
a={'id':'assign-a','instruction':'Offline UI state fixture','session_id':'worker-a','submitted':True,'taken_over':False,'owned':True,'contract_revision':1,'confirmed_revision':1,'protocol':1,'stop_requested':False,'stop_sent':False,'result':None,'inputs':[],'pending_inputs':None,'prior_results':[]}
goal=lambda i,title,assignments:{'id':i,'project_path':path,'title':title,'acceptance':'关键操作清晰，补充与验收按需出现；浅色、深色和窄窗口均可正常使用。','priority':1,'status':'active','next_step':'检查事项详情和沟通路径','blocker':'','assignments':assignments,'verdict':None,'contract_revision':1,'dependencies':[]}
state={'revision':1,'settings':{'enabled':False,'projects':[path],'provider':'grok','model':None,'reasoning_effort':None,'concurrency':2,'turn_limit':20,'dispatch_limit':10},'goals':[goal('goal-a','升级幕僚的交互体验',[a]),goal('goal-b','准备本周项目简报',[])],'run':None,'scoped_runs':[],'attempts':{},'observed':'','turns':2,'dispatches':1,'summary':'两个事项正在跟进。交互升级已进入检查，项目简报需要确认范围。','attention':None,'messages':[],'questions':[],'changes':[],'notifications':[],'requests':[],'reviews':[]}
for gid,sid,st,err in [('goal-a','review-a','running',None),('goal-b','review-b','failed','需要确认本周简报覆盖哪些项目。')]:
 state['scoped_runs'].append({'scope':gid,'run':{'id':sid,'session_id':sid,'submitted':True,'base_revision':1,'input':''},'observed':'fixture','state':st,'summary':'','error':err})
for n,st,mode,confirmed,content in [(1,'recorded','queue',False,'Recorded locally; delivery waits for follow-up.'),(2,'accepted','steer',False,'Provider accepted the steering instruction; adoption is still unconfirmed.'),(3,'accepted','queue',True,'Worker explicitly confirmed this instruction.'),(4,'unknown','steer',False,'The steering receipt is unknown; inspect the original session.')]:
 state['messages'].append({'id':f'm{n}','goal_id':'goal-a','assignment_id':'assign-a','contract_revision':1,'author':'user','content':content,'mode':mode,'state':st,'outcome':'','reply_to':None,'delivery_id':None,'confirmed':confirmed})
state['questions']=[{'id':'q-unknown','goal_id':'goal-a','assignment_id':'assign-a','contract_revision':1,'author':'worker','title':'上一条答复是否送达？','context':'Crash recovery must preserve this question.','options':[],'blocking':True,'state':'delivery_unknown','answer':'Previously recorded answer','answered_by':'user','source_input':None,'request_id':None,'answer_delivery_id':'native:claimed','factual':False}]
state['notifications']=[{'id':'notice-b','goal_id':'goal-b','kind':'failure','title':'项目简报需要确认范围','body':'请确定简报包含的项目和截止日期。','session_id':'review-b','reference_id':'review-b','read':False,'desktop':'suppressed'}]
c.execute('INSERT INTO assistant_state VALUES(1,?,?)',(state['revision'],json.dumps(state)));c.commit();c.close()
print('Seeded offline fixture while Core was stopped; follow-up disabled.')
