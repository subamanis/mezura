# mezura-expect lines=13 code=7 comments=3 extra=3 stages=1 commands=2
FROM rust:1.90 AS build

# fetch the dependencies first
WORKDIR /app
RUN cargo fetch

# then the sources
COPY . .
RUN cargo build --release

CMD ["/app/target/release/app"]
LABEL note='# not a comment'
