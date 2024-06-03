use std::net::Shutdown;
use std::sync::Arc;
use std::sync::atomic::AtomicPtr;
use cyfs_base::{AesKey, RawDecode, RawEncode, RawFixedBytes};
use crate::error::{bdt_err, BdtErrorCode, BdtResult, into_bdt_err};
use crate::executor::Executor;
use crate::stream::{Stream, StreamReader, StreamWriter};
use crate::tunnel::{TcpTunnelConnection, TunnelGuard};

pub struct TcpStreamWriter {
    tunnel: Arc<TunnelGuard>,
    send_buffer: usize,
    min_record: u16,
    max_record: u16,
    record_count: usize,
    buffer: Vec<u8>,
    buffer_pos: usize,
}

fn box_header_len() -> usize {
    u16::raw_bytes().unwrap()
}

impl TcpStreamWriter {
    pub(crate) fn new(tunnel: Arc<TunnelGuard>,
                      send_buffer: usize,
                      min_record: u16,
                      max_record: u16) -> Self {
        let record_count = send_buffer / max_record as usize + 1;
        let send_buffer = record_count * (box_header_len() + AesKey::padded_len(max_record as usize));
        Self {
            tunnel,
            send_buffer,
            min_record,
            max_record,
            record_count,
            buffer: vec![0u8; send_buffer],
            buffer_pos: 0,
        }
    }

    fn buffer_len(&self) -> usize {
        self.max_record as usize * self.record_count
    }

    fn append_to_buffer(&mut self, from: u16, data: &[u8]) {
        self.buffer[box_header_len() + from as usize..box_header_len() + from as usize + data.len()].copy_from_slice(data);
    }

    fn encode_box<'a>(&'a mut self, data: &[u8], exists_len: usize) -> &'a [u8] {
        trace!("stream {:?} encode_record {{data_len:{},exists_len:{}}}", self.tunnel.get_sequence(), data.len(), exists_len);
        let buffer_len = self.buffer.len();
        let remain_len = {
            let mut buffer = &mut self.buffer[..];
            if exists_len != 0 {
                let record_len = self.tunnel.key().enc_key.inplace_encrypt(&mut buffer[box_header_len()..], exists_len).unwrap();
                let _ = (record_len as u16).raw_encode(buffer, &None).unwrap();
                buffer = &mut buffer[box_header_len() + record_len..];
            }

            if data.len() > 0 {
                let data_len = data.len();
                let record_len = self.tunnel.key().enc_key.encrypt(data, &mut buffer[box_header_len()..], data_len).unwrap();
                let _ = (record_len as u16).raw_encode(buffer, &None).unwrap();
                buffer = &mut buffer[box_header_len() + record_len..];
            }
            buffer.len()
        };

        &self.buffer[..buffer_len - remain_len]
    }

}

#[async_trait::async_trait]
impl StreamWriter for TcpStreamWriter {
    async fn write(&mut self, buf: &[u8]) -> BdtResult<usize> {
        if self.buffer_pos == 0 && buf.len() >= self.min_record as usize {
            let buffer_len = self.buffer_len();
            let data_len = if buffer_len > buf.len() {
                buf.len()
            } else {
                buffer_len
            };

            let tunnel = self.tunnel.clone();
            let tcp_tunnel = tunnel.get_tunnel_connection::<TcpTunnelConnection>().unwrap();
            let encoded_buf = self.encode_box(&buf[..data_len], 0);
            tcp_tunnel.send(encoded_buf).await?;
            Ok(data_len)
        } else {
            let total = self.buffer_pos + buf.len();
            if total < self.min_record as usize {
                self.append_to_buffer(self.buffer_pos as u16, buf);
                self.buffer_pos += buf.len();
                Ok(buf.len())
            } else {
                let append_len = {
                    let append_len = self.max_record as usize - self.buffer_pos;
                    if append_len > buf.len() {
                        buf.len()
                    } else {
                        append_len
                    }
                };
                self.append_to_buffer(self.buffer_pos as u16, &buf[..append_len]);
                self.buffer_pos += append_len;

                let tunnel = self.tunnel.clone();
                let tcp_tunnel = tunnel.get_tunnel_connection::<TcpTunnelConnection>().unwrap();
                let encoded_buf = self.encode_box(&[], self.buffer_pos);
                tcp_tunnel.send(encoded_buf).await?;
                self.buffer_pos = 0;

                Ok(append_len)
            }
        }
    }

    async fn write_all(&mut self, buf: &[u8]) -> BdtResult<()> {
        let mut remaind_buf = buf;
        while remaind_buf.len() > 0 {
            let sent = self.write(remaind_buf).await?;
            remaind_buf = &remaind_buf[sent..];
        }
        Ok(())
    }

    async fn flush(&mut self) -> BdtResult<()> {
        let tunnel = self.tunnel.clone();
        let tcp_tunnel = tunnel.get_tunnel_connection::<TcpTunnelConnection>().unwrap();
        let encoded_buf = self.encode_box(&[], self.buffer_pos);
        tcp_tunnel.send(encoded_buf).await?;
        tcp_tunnel.flush().await?;
        self.buffer_pos = 0;
        Ok(())
    }
}

pub struct TcpStreamReader {
    tunnel: Arc<TunnelGuard>,
    max_record: u16,
    buffer: Vec<u8>,
    buffer_pos: usize,
    buffer_data_len: usize,
}

impl TcpStreamReader {
    fn new(tunnel: Arc<TunnelGuard>, max_record: u16) -> Self {
        let send_buffer = AesKey::padded_len(max_record as usize);
        Self {
            tunnel,
            max_record,
            buffer: vec![0u8; send_buffer],
            buffer_pos: 0,
            buffer_data_len: 0,
        }
    }

    async fn recv_box(&mut self, buffer: Option<&mut [u8]>) -> BdtResult<usize> {
        let buffer = if buffer.is_some() {
            buffer.unwrap()
        } else {
            self.buffer.as_mut()
        };
        let tunnel = self.tunnel.clone();
        let tcp_tunnel = tunnel.get_tunnel_connection::<TcpTunnelConnection>().unwrap();
        let mut header_buf = [0u8; 16];
        tcp_tunnel.recv_exact(&mut header_buf[..box_header_len()]).await?;
        let (len, _) = u16::raw_decode(header_buf.as_slice()).map_err(into_bdt_err!(BdtErrorCode::RawCodecError, "tunnel {:?} raw decode box", self.tunnel.get_sequence()))?;
        let len = len as usize;
        if len > buffer.len() - box_header_len() {
            return Err(bdt_err!(BdtErrorCode::InvalidData, "tunnel {:?} recv unexpect box. len {}", self.tunnel.get_sequence(), len));
        }
        tcp_tunnel.recv_exact(&mut buffer[..len]).await?;
        let len = self.tunnel.key().enc_key.inplace_decrypt(&mut buffer[..len], len).map_err(into_bdt_err!(BdtErrorCode::CryptoError))?;
        Ok(len)
    }
}

#[async_trait::async_trait]
impl StreamReader for TcpStreamReader {
    async fn read(&mut self, buf: &mut [u8]) -> BdtResult<usize> {
        if self.buffer_data_len == 0 && self.buffer.len() <= buf.len() {
            self.recv_box(Some(buf)).await
        } else {
            if self.buffer_data_len == 0 {
                let len = self.recv_box(None).await?;
                self.buffer_data_len = len;
                self.buffer_pos = 0;
            }

            if self.buffer_data_len != 0 && self.buffer_pos != self.buffer_data_len {
                return if self.buffer_data_len - self.buffer_pos > buf.len() {
                    buf.copy_from_slice(&self.buffer[self.buffer_pos..self.buffer_pos + buf.len()]);
                    self.buffer_pos += buf.len();
                    Ok(buf.len())
                } else {
                    buf[..self.buffer_data_len - self.buffer_pos].copy_from_slice(&self.buffer[self.buffer_pos..]);
                    self.buffer_pos = 0;
                    self.buffer_data_len = 0;
                    Ok(self.buffer_data_len - self.buffer_pos)
                }
            }

            Ok(0)
        }
    }

    async fn read_exact(&mut self, buf: &mut [u8]) -> BdtResult<()> {
        let mut buffer = buf;
        while buffer.len() > 0 {
            let len = self.read(buffer).await?;
            buffer = &mut buffer[len..];
        }
        Ok(())
    }
}

pub struct TcpStream {
    reader: TcpStreamReader,
    writer: TcpStreamWriter,
    tunnel: Arc<TunnelGuard>,
}

impl TcpStream {
    pub fn new(tunnel: TunnelGuard,
               send_buffer: usize,
               min_box: u16,
               max_box: u16) -> Self {
        let tunnel = Arc::new(tunnel);
        Self {
            reader: TcpStreamReader::new(tunnel.clone(), max_box),
            writer: TcpStreamWriter::new(tunnel.clone(), send_buffer, min_box, max_box),
            tunnel,
        }
    }
}

impl Drop for TcpStream {
    fn drop(&mut self) {
        Executor::block_on(async move {
            if let Err(e) = self.writer.flush().await {
                log::error!("stream {:?} err {}", self.tunnel.get_sequence(), e);
            }
            if let Err(e) = self.tunnel.shutdown(Shutdown::Both).await {
                log::error!("stream {:?} err {}", self.tunnel.get_sequence(), e);
            }
        })
    }
}
#[async_trait::async_trait]
impl Stream for TcpStream {
    async fn write(&mut self, buf: &[u8]) -> BdtResult<usize> {
        self.writer.write(buf).await
    }

    async fn write_all(&mut self, buf: &[u8]) -> BdtResult<()> {
        self.writer.write_all(buf).await
    }

    async fn flush(&mut self) -> BdtResult<()> {
        self.writer.flush().await
    }

    async fn read(&mut self, buf: &mut [u8]) -> BdtResult<usize> {
        self.reader.read(buf).await
    }

    async fn read_exact(&mut self, buf: &mut [u8]) -> BdtResult<()> {
        self.reader.read_exact(buf).await
    }

    async fn shutdown(&self, how: Shutdown) -> BdtResult<()> {
        self.tunnel.shutdown(how).await
    }
}
