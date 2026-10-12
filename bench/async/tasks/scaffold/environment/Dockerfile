FROM python:3.12-slim

# The tools (/opt/async/bin) and their library; the ledger and the tools' state
# live in /var/lib/async, outside the working directory.
COPY async/ /opt/async/
RUN chmod 755 /opt/async/bin/* && mkdir -p /app /var/lib/async/state
COPY app/ /app/
ENV PATH=/opt/async/bin:$PATH
WORKDIR /app
