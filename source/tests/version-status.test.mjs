import test from 'node:test';
import assert from 'node:assert/strict';
import {compareVersions,agentVersionLabel,updateNotice} from '../src/version-status.mjs';
const release={current:'0.11.5',latest:'0.11.6',agentCurrent:'0.2.3',agentLatest:'0.2.3',stale:false,available:true};
test('numeric versions exclude unknown versions and downgrades',()=>{
 assert.equal(compareVersions('0.10.0','0.9.9'),1);assert.equal(compareVersions('0.2.3','0.2.3'),0);
 assert.equal(compareVersions('0.2.3','0.2.4'),-1);
 for(const v of ['', 'unknown','0.2.3-rc1','9.1','1.2.999999'])assert.equal(compareVersions(v,'0.2.3'),null);
});
test('agent labels distinguish successful checks from failures and future compatibility',()=>{
 assert.equal(agentVersionLabel({agentVersion:'0.2.3'},release,'0.2.3'),'已是最新版本');
 assert.match(agentVersionLabel({agentVersion:'0.2.2'},release,'0.2.3'),/可升级至/);
 assert.match(agentVersionLabel({agentVersion:'0.2.4'},release,'0.2.3'),/高于发布版/);
 assert.match(agentVersionLabel({agentVersion:'0.2.3'},{...release,stale:true,error:'offline'},'0.2.3'),/检测暂不可用/);
 assert.match(agentVersionLabel({agentVersion:'0.2.3'},{...release,agentLatest:'0.2.4'},'0.2.3'),/先更新主控/);
});
test('automatic notices include real newer releases and stay quiet on failed checks',()=>{
 assert.equal(updateNotice({...release,available:false},[{agentVersion:'0.2.3'}]),'');
 assert.match(updateNotice(release,[{agentVersion:'0.2.2'},{agentVersion:'0.2.3'}]),/主控 0.11.6.*Agent 0.2.3（1 台）/);
 assert.equal(updateNotice({...release,stale:true},[{agentVersion:'0.2.2'}]),'');
});
