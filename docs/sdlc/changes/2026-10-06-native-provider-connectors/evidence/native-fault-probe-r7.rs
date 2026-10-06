

struct ParentBrokenWriter;
impl tokio::io::AsyncWrite for ParentBrokenWriter {
    fn poll_write(self: std::pin::Pin<&mut Self>, _: &mut std::task::Context<'_>, _: &[u8]) -> std::task::Poll<std::io::Result<usize>> {
        std::task::Poll::Ready(Err(std::io::Error::new(std::io::ErrorKind::BrokenPipe,"injected write failure")))
    }
    fn poll_flush(self: std::pin::Pin<&mut Self>, _: &mut std::task::Context<'_>) -> std::task::Poll<std::io::Result<()>> {std::task::Poll::Ready(Ok(()))}
    fn poll_shutdown(self: std::pin::Pin<&mut Self>, _: &mut std::task::Context<'_>) -> std::task::Poll<std::io::Result<()>> {std::task::Poll::Ready(Ok(()))}
}

#[tokio::test]
async fn parent_probe_writer_failure_resolves_queued_request_without_reader_eof() {
    let (reader,_keep_peer_open)=tokio::io::duplex(1024);
    let connection=Connection::new(reader,ParentBrokenWriter,Arc::new(RecordingHandler::default()));
    let runtime=AcpRuntime::new(AcpClient::new(connection,None));
    let result=tokio::time::timeout(std::time::Duration::from_millis(250),runtime.send_turn("backend-1",vec![RuntimeContent::text("must become unknown")] )).await;
    assert!(matches!(result,Ok(TurnOutcome::Unknown(_))|Ok(TurnOutcome::NotSent(_))),"writer failure must complete the waiting request as unknown/unsent without waiting for unrelated reader EOF; got {result:?}");
}

#[tokio::test]
async fn parent_probe_ambiguous_rpc_error_is_not_proof_of_rejection_before_execution() {
    let (runtime,seen)=runtime("error");runtime.initialize().await.unwrap();
    let result=runtime.send_turn("backend-1",vec![RuntimeContent::text("work may have happened")]).await;
    assert!(seen.lock().unwrap().iter().any(|m|m=="session/prompt"));
    assert!(matches!(result,TurnOutcome::Unknown(_)),"generic provider RPC error cannot prove no side effects or no live writer; got {result:?}");
}
