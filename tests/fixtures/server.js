const http = require('http');
const fs = require('fs');
const path = require('path');

const page = fs.readFileSync(path.join(__dirname, 'fullscreen.html'));
http.createServer((_request, response) => {
  response.writeHead(200, { 'Content-Type': 'text/html; charset=utf-8' });
  response.end(page);
}).listen(8765, '127.0.0.1');
