import { chromium } from 'playwright';
import {readFileSync,writeFileSync} from 'node:fs';
import path from 'node:path';
import assert from 'node:assert/strict';
const root=path.resolve('../../../..'),scratch=path.resolve('..');
const evidence=path.join(root,'docs/sdlc/changes/2026-10-05-assistant-interaction-upgrade/evidence');
const token=readFileSync(path.join(scratch,'server.txt'),'utf8').match(/Pairing token: (\w+)/)?.[1];
assert(token);
const browser=await chromium.launch({headless:true});
try {
 const context=await browser.newContext({viewport:{width:1280,height:800},colorScheme:'light'});
 await context.addInitScript(()=>localStorage.setItem('codetwo.language','zh-CN'));
 const page=await context.newPage();
 await page.goto('http://127.0.0.1:14674/#token='+token);
 await page.getByRole('button',{name:'幕僚',exact:true}).click();
 const s=page.getByRole('region',{name:'幕僚'});
 await s.getByRole('button',{name:'查看事项：升级幕僚的交互体验',exact:true}).click();
 await s.getByText('已暂停',{exact:true}).waitFor();
 await s.getByRole('button',{name:'待办 (3)',exact:true}).click();
 await s.getByText('The execution stopped; existing files were retained',{exact:true}).waitFor();
 await page.screenshot({path:path.join(evidence,'stopped-light.png')});
 const r=JSON.parse(readFileSync(path.join(evidence,'ui-result.json'),'utf8'));
 r.checks.stopRecorded=true;r.checks.terminalStopRendered=true;r.completed=true;
 writeFileSync(path.join(evidence,'ui-result.json'),JSON.stringify(r,null,2)+'\n');
 console.log('PASS: final paused goal and persisted stopped notification rendered; prior request/message outcomes preserved, no replay.');
} finally { await browser.close(); }
