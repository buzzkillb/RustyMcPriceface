FROM python:3.12-alpine

RUN addgroup -g 1001 app && adduser -u 1001 -G app -s /bin/sh -D app

WORKDIR /app

RUN apk add --no-cache \
    curl \
    libffi \
    fontconfig \
    freetype \
    libstdc++

COPY requirements.txt .
RUN pip install --no-cache-dir -r requirements.txt

COPY *.py .

RUN chown -R app:app /app

USER app

ENV PYTHONUNBUFFERED=1

CMD ["python", "bot.py"]
