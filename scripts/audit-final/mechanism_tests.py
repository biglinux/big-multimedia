#!/usr/bin/env python3
"""Real OS/curl/ffmpeg mechanisms with synthetic data. NOT Rust implementation tests."""
import json,os,subprocess,tempfile,threading,time,unittest,shutil,fcntl
from pathlib import Path
from http.server import BaseHTTPRequestHandler,ThreadingHTTPServer

def config(name,value):
    escapes={'\\':'\\\\','"':'\\"','\n':'\\n','\r':'\\r','\t':'\\t','\v':'\\v'}
    return name+' = "'+''.join(escapes.get(c,c) for c in value)+'"\n'

@unittest.skipUnless(shutil.which('curl'), 'curl unavailable; no installation attempted')
class CurlMechanisms(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        class Handler(BaseHTTPRequestHandler):
            calls=[]
            def log_message(self,*a):pass
            def do_GET(self):self.reply()
            def do_POST(self):self.reply()
            def reply(self):
                body=self.rfile.read(int(self.headers.get('Content-Length','0')))
                self.calls.append(self.path)
                value=json.dumps({'path':self.path,'body':body.decode('utf-8'),'token':self.headers.get('Authorization')}).encode()
                self.send_response(200);self.send_header('Content-Length',str(len(value)));self.end_headers()
                time.sleep(.08);self.wfile.write(value)
        cls.handler=Handler;cls.server=ThreadingHTTPServer(('127.0.0.1',0),Handler)
        cls.worker=threading.Thread(target=cls.server.serve_forever,daemon=True);cls.worker.start()
        cls.url=f'http://127.0.0.1:{cls.server.server_port}/data?token=synthetic-private-value'
    @classmethod
    def tearDownClass(cls):cls.server.shutdown();cls.server.server_close();cls.worker.join(2)
    def invoke(self, cfg, env=None):
        argv=['curl','--disable','--silent','--show-error','--globoff','--proto','=http,https','--max-time','3','--config','-']
        with subprocess.Popen(argv,stdin=subprocess.PIPE,stdout=subprocess.PIPE,stderr=subprocess.PIPE,env=env) as child:
            cmdline=Path(f'/proc/{child.pid}/cmdline').read_bytes()
            self.assertNotIn(b'synthetic-private-value',cmdline)
            out,err=child.communicate(cfg.encode(),timeout=5)
            self.assertEqual(child.returncode,0,err.decode());return json.loads(out)
    def test_private_url_and_header_off_argv(self):
        r=self.invoke(config('url',self.url)+config('header','Authorization: Bearer synthetic-private-value'))
        self.assertEqual(r['token'],'Bearer synthetic-private-value')
    def test_literal_body_not_local_file_or_new_directive(self):
        body='@private-file\n"quoted" \\ slash\ttext\nurl = "http://invalid.example/"'
        r=self.invoke(config('url',self.url)+config('data-raw',body));self.assertEqual(r['body'],body)
    def test_user_curlrc_is_disabled(self):
        with tempfile.TemporaryDirectory() as d:
            Path(d,'.curlrc').write_text('url = "file:///do-not-open"\n')
            r=self.invoke(config('url',self.url),dict(os.environ,CURL_HOME=d));self.assertTrue(r['path'].startswith('/data?'))
    def test_glob_is_not_multiple_requests(self):
        before=len(self.handler.calls);self.invoke(config('url',self.url+'&a=[1-3]'));self.assertEqual(len(self.handler.calls),before+1)

class StorageMechanisms(unittest.TestCase):
    def test_no_replace_publishes_once(self):
        with tempfile.TemporaryDirectory() as d:
            p=Path(d);temp=p/'private';temp.mkdir(mode=0o700);stage=temp/'x';stage.write_bytes(b'new');os.chmod(stage,0o600)
            out=p/'out';os.link(stage,out)
            with self.assertRaises(FileExistsError):os.link(stage,out)
            stage.unlink();temp.rmdir();self.assertEqual(out.read_bytes(),b'new')
    def test_existing_target_survives_failed_publication_and_cleanup(self):
        with tempfile.TemporaryDirectory() as d:
            p=Path(d);target=p/'output';target.write_bytes(b'original');stage=p/'stage';stage.write_bytes(b'new')
            with self.assertRaises(FileExistsError):os.link(stage,target)
            stage.unlink();self.assertEqual(target.read_bytes(),b'original')
    def test_lock_same_inode_nonblocking(self):
        with tempfile.TemporaryDirectory() as d:
            with open(Path(d)/'state.lock','w') as one,open(Path(d)/'state.lock','r+') as two:
                fcntl.flock(one,fcntl.LOCK_EX|fcntl.LOCK_NB)
                with self.assertRaises(BlockingIOError):fcntl.flock(two,fcntl.LOCK_EX|fcntl.LOCK_NB)
    def test_terminal_symlink_read_is_rejected(self):
        with tempfile.TemporaryDirectory() as d:
            p=Path(d);(p/'data').write_bytes(b'bytes');(p/'link').symlink_to(p/'data')
            with self.assertRaises(OSError):os.open(p/'link',os.O_RDONLY|os.O_NOFOLLOW|os.O_NONBLOCK)

@unittest.skipUnless(shutil.which('ffmpeg') and shutil.which('ffprobe'),'ffmpeg/ffprobe unavailable; no installation attempted')
class FFmpegMechanisms(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.temp=tempfile.TemporaryDirectory();cls.root=Path(cls.temp.name);cls.media=cls.root/'original.mkv'
        cmd=['ffmpeg','-nostdin','-v','error','-filter_threads','1','-filter_complex_threads','1',
             '-f','lavfi','-i','testsrc=size=64x64:rate=5','-f','lavfi','-i','sine=frequency=440:sample_rate=8000',
             '-t','1.6','-map','0:v:0','-map','1:a:0','-c:v','mpeg4','-threads','1','-c:a','pcm_s16le','-progress','pipe:1','--',str(cls.media)]
        cls.completed=subprocess.run(cmd,capture_output=True,text=True,timeout=15)
        if cls.completed.returncode:raise AssertionError(cls.completed.stderr)
    @classmethod
    def tearDownClass(cls):cls.temp.cleanup()
    def probe(self,path):
        r=subprocess.run(['ffprobe','-v','error','-show_streams','-show_format','-of','json','--',str(path)],capture_output=True,text=True,timeout=5)
        self.assertEqual(r.returncode,0,r.stderr);return json.loads(r.stdout)
    def test_probe_exposes_audio_video_and_duration(self):
        r=self.probe(self.media);self.assertEqual({s['codec_type'] for s in r['streams']},{'audio','video'});self.assertGreater(float(r['format']['duration']),1)
    def test_progress_legacy_out_time_ms_is_microseconds(self):
        fields={}
        for line in self.completed.stdout.splitlines():
            if '=' in line:k,v=line.split('=',1);fields[k]=v
        self.assertEqual(fields['progress'],'end')
        self.assertAlmostEqual(float(fields['out_time_ms'])/1_000_000,1.6,delta=.3)
    def test_concat_private_stages_maps_all_streams(self):
        listing=self.root/'parts.ffconcat';listing.write_text(f"file '{self.media}'\nfile '{self.media}'\n")
        out=self.root/'joined.mkv'
        r=subprocess.run(['ffmpeg','-nostdin','-v','error','-f','concat','-safe','0','-i',str(listing),'-map','0','-c','copy','--',str(out)],capture_output=True,text=True,timeout=10)
        self.assertEqual(r.returncode,0,r.stderr);value=self.probe(out)
        self.assertEqual({s['codec_type'] for s in value['streams']},{'audio','video'});self.assertAlmostEqual(float(value['format']['duration']),3.2,delta=.3)
    def test_audio_only_is_distinguishable_from_a_video_export(self):
        out=self.root/'audio.wav';r=subprocess.run(['ffmpeg','-nostdin','-v','error','-i',str(self.media),'-vn','-c:a','pcm_s16le','--',str(out)],capture_output=True,text=True,timeout=5)
        self.assertEqual(r.returncode,0,r.stderr);self.assertEqual([s['codec_type'] for s in self.probe(out)['streams']],['audio'])

if __name__=='__main__':unittest.main(verbosity=2)
