FROM python:3.12-alpine

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

ENV PYTHONUNBUFFERED=1

CMD ["python", "bot.py"]
