import test from 'node:test';
import assert from 'node:assert/strict';
import {seasonFrame,SEASON_MS,TRANSITION_MS,seasonMetricColor} from '../src/seasons.mjs';
import {terminalAppearance} from '../src/terminal-appearance.mjs';
test('four 60-second slots wrap winter into spring',()=>{
 assert.equal(SEASON_MS,60000);assert.equal(TRANSITION_MS,6000);
 for(let cycle=0;cycle<3;cycle++)for(let season=0;season<4;season++){
  const start=(cycle*4+season)*60000;
  assert.equal(seasonFrame(start).index,season);
  assert.equal(seasonFrame(start+59999).index,season);
  assert.equal(seasonFrame(start+60000).index,(season+1)%4);
 }
});
test('soft painting transition holds, eases and reaches the next scene',()=>{
 for(let season=0;season<4;season++){
  const t=season*60000;
  assert.equal(seasonFrame(t+53999).blend,0);
  assert.equal(seasonFrame(t+54000).blend,0);
  assert.equal(seasonFrame(t+57000).blend,.5);
  assert(seasonFrame(t+59999).blend>.999999);
  assert.equal(seasonFrame(t+60000).blend,0);
  assert.equal(seasonFrame(t+59999).next,seasonFrame(t+60000).index);
 }
});
test('invalid clock readings cannot escape the four bounded seasons',()=>{
 for(const t of [-1,NaN,Infinity,-Infinity])assert.deepEqual(seasonFrame(t),seasonFrame(0));
 for(let t=0;t<480000;t+=139){const f=seasonFrame(t);assert(f.index>=0&&f.index<4&&f.next>=0&&f.next<4&&f.blend>=0&&f.blend<=1);}
});
test('seasonal metric colors join continuously and keep dark-mode contrast',()=>{
 for(const dark of [false,true])for(let index=0;index<4;index++){
  const next=(index+1)%4,start=seasonMetricColor({index,next,blend:0},dark),end=seasonMetricColor({index,next,blend:1},dark);
  assert.match(start,/^#[0-9a-f]{6}$/);assert.notEqual(start,end);
  assert.equal(end,seasonMetricColor({index:next,next:(next+1)%4,blend:0},dark));
 }
});

test('every site theme retains the same opaque terminal palette',()=>{
 const expected=terminalAppearance();
 assert.deepEqual(expected,{background:'#183447',foreground:'#dce6f0',cursor:'#b9dbef',selectionBackground:'#406584'});
 assert.equal(expected.background,'#183447');assert.equal(expected.foreground,'#dce6f0');
 for(const appearance of ['default','clear','sketch','anime','seasons','untrusted'])
  for(const theme of ['light','dark'])
   for(const themePackage of ['', 'sample'])assert.deepEqual(terminalAppearance({dataset:{appearance,theme,themePackage}}),expected);
 const changed=terminalAppearance();changed.background='#fff';assert.deepEqual(terminalAppearance(),expected);
});
