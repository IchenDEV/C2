import subprocess,sys,time
exe,log=sys.argv[1],sys.argv[2]
extra=sys.argv[3:]
t=time.time()
with open(log,"wb") as f:
    p=subprocess.Popen([exe]+extra,stdout=f,stderr=subprocess.STDOUT,start_new_session=True)
    try:
        rc=p.wait(timeout=60)
    except subprocess.TimeoutExpired:
        import os,signal
        os.killpg(p.pid,signal.SIGKILL); rc="TIMEOUT"
print(exe.split('/')[-1],"rc",rc,"%.1fs"%(time.time()-t))
