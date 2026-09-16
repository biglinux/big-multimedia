#!/usr/bin/env python3
"""Actual remote assets in Chromium, mocked HTTP transport. NOT Rust/GTK/server tests."""
import argparse,json,shutil
from pathlib import Path
from playwright.sync_api import sync_playwright

MOCK=r'''() => {
 window.audit={playing:false,volume:50,commands:[],fail:false,active:0,peak:0,gets:0};
 window.fetch=async (url, options) => {
  const a=window.audit; const path=new URL(url).pathname;
  if(options.method==='GET'){a.active++;a.peak=Math.max(a.peak,a.active);a.gets++;}
  await new Promise(resolve=>setTimeout(resolve,50));
  if(options.method==='GET')a.active--;
  if(options.signal.aborted)throw new DOMException('aborted','AbortError');
  if(options.headers['X-Big-Media-Remote']!=='1')throw Error('missing command header');
  if(a.fail)return new Response('unavailable',{status:503});
  if(options.method==='POST'){
   a.commands.push(String(url));
   if(path.endsWith('/volume'))a.volume=Number(new URL(url).searchParams.get('v'));
   if(path.endsWith('/play-pause'))a.playing=!a.playing;
   return new Response('ok',{status:200});
  }
  return new Response(JSON.stringify({playing:a.playing,volume:a.volume,title:'Canção <b>segura</b>',artist:'Artista',position:10,duration:100}),{status:200});
 };
}'''

def main():
 parser=argparse.ArgumentParser(description=__doc__);parser.add_argument('source',type=Path);parser.add_argument('output',type=Path);args=parser.parse_args()
 args.output.mkdir(parents=True,exist_ok=True);assets=args.source/'crates/app/big-app-kit-core/src';checks=[];errors=[]
 def check(name,value):
  checks.append({'name':name,'passed':bool(value)});assert value,name
 def install(page):
  # Navigation is blocked by this environment's browser policy. Render local
  # assets without any navigation/network, with a base-URI facade for the mock.
  import re
  html=(assets/'remote_control.html').read_text()
  html=re.sub(r'<link[^>]*>|<script.*?</script>','',html,flags=re.S)
  page.set_content(html)
  page.add_style_tag(content=(assets/'remote_control.css').read_text())
  page.evaluate(MOCK)
  page.evaluate("source => (new Function('window', source))({location:{href:'http://remote.invalid/token/'}})", (assets/'remote_control.js').read_text())
 try:
  with sync_playwright() as p:
   executable=shutil.which('chromium') or shutil.which('chromium-browser')
   if not executable:raise RuntimeError('Chromium unavailable; no installation attempted')
   browser=p.chromium.launch(executable_path=executable,headless=True,args=['--no-sandbox','--disable-dev-shm-usage'])
   ctx=browser.new_context(locale='pt-BR',viewport={'width':390,'height':844})
   page=ctx.new_page();page.on('pageerror',lambda e:errors.append(str(e)));install(page);page.get_by_text('Conectado ao computador',exact=True).wait_for()
   check('pt-BR document language',page.locator('html').get_attribute('lang')=='pt-BR')
   check('untrusted title stays text',page.locator('#title').inner_text()=='Canção <b>segura</b>' and page.locator('#title b').count()==0)
   page.get_by_role('button',name='Reproduzir',exact=True).click();page.get_by_role('button',name='Pausar',exact=True).wait_for()
   check('play command is one POST',page.evaluate("audit.commands.filter(x=>x.endsWith('/play-pause')).length") == 1)
   page.evaluate("() => {const e=document.getElementById('vol');for(const v of [10,20,30,75]){e.value=v;e.dispatchEvent(new Event('input',{bubbles:true}));}}")
   page.wait_for_timeout(600)
   check('volume changes coalesced',page.evaluate("audit.volume===75 && audit.commands.filter(x=>x.includes('/volume?')).length===1"))
   for width in (320,390,1024):
    page.set_viewport_size({'width':width,'height':844});page.wait_for_timeout(80)
    check(f'no horizontal overflow at {width}px',page.evaluate('document.documentElement.scrollWidth<=innerWidth'))
    page.screenshot(path=str(args.output/f'remote-{width}.png'),full_page=True)
   check('zoom not disabled','maximum-scale' not in page.locator('meta[name=viewport]').get_attribute('content'))
   page.set_viewport_size({'width':390,'height':844});page.add_style_tag(content='html{font-size:200% !important}')
   check('text zoom reflows',page.evaluate('document.documentElement.scrollWidth<=innerWidth'))
   page.screenshot(path=str(args.output/'remote-text-200.png'),full_page=True)
   page.evaluate("Object.defineProperty(document,'hidden',{configurable:true,get:()=>true});document.dispatchEvent(new Event('visibilitychange'))")
   page.wait_for_timeout(150);before=page.evaluate('audit.gets');page.wait_for_timeout(1200)
   check('simulated hidden page stops polling',page.evaluate('audit.gets')==before)
   page.evaluate("audit.fail=true;Object.defineProperty(document,'hidden',{configurable:true,get:()=>false});document.dispatchEvent(new Event('visibilitychange'))")
   page.get_by_text('Sem conexão. Verifique a rede e mantenha o player aberto.',exact=True).wait_for()
   check('disconnection disables actions',page.locator('#pp').is_disabled() and page.locator('#vol').is_disabled())
   page.evaluate('audit.fail=false');page.get_by_text('Conectado ao computador',exact=True).wait_for(timeout=6000)
   check('status requests do not overlap',page.evaluate('audit.peak')==1)
   tree=page.locator('main').aria_snapshot();(args.output/'aria-tree.txt').write_text(tree)
   check('accessible tree exposes slider and buttons','slider' in tree and 'button' in tree)
   check('unencrypted local network warning visible',page.get_by_text('Use somente em uma rede local confiável. Esta conexão HTTP não é criptografada.',exact=True).is_visible())
   en=browser.new_context(locale='en-US');ep=en.new_page();install(ep);ep.get_by_text('Connected to your computer',exact=True).wait_for()
   check('English document language',ep.locator('html').get_attribute('lang')=='en')
   check('no uncaught JS exceptions',not errors);browser.close()
 finally:
  (args.output/'result.json').write_text(json.dumps({'scope':'Actual Chromium/local assets; mock fetch and window.location facade; no network, CSP, Rust server or GTK execution','checks':checks,'errors':errors},ensure_ascii=False,indent=2))
 print(json.dumps({'passed':len(checks),'failed':sum(not c['passed'] for c in checks)},indent=2))

if __name__=='__main__':main()
