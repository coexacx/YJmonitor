import test from 'node:test';
import assert from 'node:assert/strict';
import {compactBytes,workspaceWidth,workspaceHeight} from '../src/terminal-monitor.mjs';
test('disk units preserve unknown values and readable K/M/G boundaries',()=>{
 for(const n of [undefined,null,NaN,-1,Infinity])assert.equal(compactBytes(n),'—');
 assert.equal(compactBytes(0),'0B');assert.equal(compactBytes(1024),'1K');
 assert.equal(compactBytes(1536*1024),'1.5M');assert.equal(compactBytes(5*1024**3),'5G');
});
test('workspace separators preserve minimum sidebar and terminal space',()=>{
 assert.equal(workspaceWidth(-20,1200),220);assert.equal(workspaceWidth(1000,1000),520);
 assert.equal(workspaceWidth(1000,701),321);assert.equal(workspaceWidth(50,390),150);assert.equal(workspaceWidth(999,390),235);assert.equal(workspaceHeight(-5,700),150);
 assert.equal(workspaceHeight(9999,700),460);assert.equal(workspaceHeight(220,700),220);
});
