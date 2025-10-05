macro_rules! multiio {
    ( $name:ident { $( $v:ident ( $x:ty ) , )* } ) => {
        ::paste::paste! {
            ::pin_project_lite::pin_project! {
                #[project = [<$name Proj>]]
                pub enum $name {
                    $( $v { #[pin] inner: $x }, )*
                }
            }

            #[allow(non_snake_case)]
            mod [<$name _multiio >] {
                use ::std::io::IoSlice;
                use ::std::pin::Pin;
                use ::std::task::{Context, Poll};
                use ::tokio::io::{AsyncRead, AsyncWrite, ReadBuf};
                use super:: $name as This;
                use super:: [< $name Proj >] as ThisProj;

                impl AsyncRead for This {
                    fn poll_read(self: Pin<&mut Self>, cx: &mut Context<'_>, buf: &mut ReadBuf<'_>) -> Poll<std::io::Result<()>> {
                        match self.project() {
                            $(
                                ThisProj:: $v { inner } => inner.poll_read(cx, buf),
                            )*
                        }
                    }
                }

                impl AsyncWrite for This {
                    fn poll_write(self: Pin<&mut Self>, cx: &mut Context<'_>, buf: &[u8]) -> Poll<Result<usize, std::io::Error>> {
                        match self.project() {
                            $(
                                ThisProj:: $v { inner } => inner.poll_write(cx, buf),
                            )*
                        }
                    }

                    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), std::io::Error>> {
                        match self.project() {
                            $(
                                ThisProj:: $v { inner } => inner.poll_flush(cx),
                            )*
                        }
                    }

                    fn poll_shutdown(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), std::io::Error>> {
                        match self.project() {
                            $(
                                ThisProj:: $v { inner } => inner.poll_shutdown(cx),
                            )*
                        }
                    }

                    fn poll_write_vectored(self: Pin<&mut Self>, cx: &mut Context<'_>, bufs: &[IoSlice<'_>]) -> Poll<Result<usize, std::io::Error>> {
                        match self.project() {
                            $(
                                ThisProj:: $v { inner } => inner.poll_write_vectored(cx, bufs),
                            )*
                        }
                    }

                    fn is_write_vectored(&self) -> bool {
                        match self {
                            $(
                                Self:: $v { inner } => inner.is_write_vectored(),
                            )*
                        }
                    }
                }
            }

            $(
                impl From< $x > for $name {
                    fn from(inner: $x ) -> Self {
                        Self:: $v { inner }
                    }
                }
            )*
        }
    };
}

pub(crate) use multiio;
