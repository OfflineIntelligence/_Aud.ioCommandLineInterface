# File: ChatInterface.py
import http.client
import json
import sys
import re
import time
from typing import List, Dict, Any, Generator, Optional

HOST = "localhost"
PORT = 8000
TIMEOUT = 120

class StreamingChatClient:
    def __init__(self, host="localhost", port=8000):
        self.host = host
        self.port = port
        
    def send_message_stream(self, messages: List[Dict[str, str]]) -> Generator[str, None, None]:
        """Send message and stream response with proper SSE handling"""
        conn = http.client.HTTPConnection(self.host, self.port, timeout=TIMEOUT)
        
        payload = json.dumps({
            "model": "local-llm",
            "messages": messages,
            "max_tokens": 2000,
            "stream": True,
            "temperature": 0.7
        })
        
        headers = {
            'Content-Type': 'application/json',
            'Accept': 'text/event-stream',
            'Cache-Control': 'no-cache',
            'Connection': 'keep-alive'
        }
        
        try:
            conn.request("POST", "/generate/stream", payload, headers)
            response = conn.getresponse()
            
            if response.status != 200:
                data = response.read()
                raise Exception(f"HTTP {response.status}: {data.decode()}")
            
            buffer = b""
            while True:
                # Read one byte at a time for real-time streaming
                chunk = response.read(1)
                if not chunk:
                    break
                
                buffer += chunk
                
                if b'\n' in buffer:
                    line, buffer = buffer.split(b'\n', 1)
                    content = self._process_sse_line(line.strip())
                    if content is not None:
                        yield content
                        
            if buffer:
                content = self._process_sse_line(buffer)
                if content is not None:
                    yield content
                    
        except Exception as e:
            raise Exception(f"Stream error: {e}")
        finally:
            conn.close()
    
    def _process_sse_line(self, line: bytes) -> Optional[str]:
        """Process SSE line with robust error handling"""
        if not line or line == b'[DONE]':
            return None
            
        try:
            # Handle SSE format: "data: {json}"
            if line.startswith(b'data: '):
                json_str = line[6:].decode('utf-8').strip()
                if json_str == '[DONE]':
                    return None
                    
                data = json.loads(json_str)
                if data.get('choices') and len(data['choices']) > 0:
                    delta = data['choices'][0].get('delta', {})
                    return delta.get('content', '')
                    
            elif line.startswith(b'{'):
                data = json.loads(line.decode('utf-8'))
                if data.get('choices') and len(data['choices']) > 0:
                    message = data['choices'][0].get('message', {})
                    return message.get('content', '')
                    
        except json.JSONDecodeError:
            # Try to extract content with regex as last resort
            try:
                line_str = line.decode('utf-8', errors='ignore')
                if '"content":' in line_str:
                    match = re.search(r'"content":\s*"([^"]*)"', line_str)
                    if match:
                        content = match.group(1)
                        # Unescape JSON characters
                        content = content.replace('\\n', '\n').replace('\\"', '"').replace('\\\\', '\\')
                        return content
            except:
                pass
                
        return None
    
    def send_message_non_streaming(self, messages: List[Dict[str, str]]) -> str:
        """Non-streaming fallback"""
        conn = http.client.HTTPConnection(self.host, self.port, timeout=TIMEOUT)
        
        payload = json.dumps({
            "model": "local-llm",
            "messages": messages,
            "max_tokens": 2000,
            "stream": False,
            "temperature": 0.7
        })
        
        headers = {'Content-Type': 'application/json'}
        
        try:
            conn.request("POST", "/generate", payload, headers)  # Non-streaming endpoint
            response = conn.getresponse()
            
            if response.status != 200:
                data = response.read()
                raise Exception(f"HTTP {response.status}: {data.decode()}")
            
            data = response.read()
            response_data = json.loads(data.decode('utf-8'))
            
            if response_data.get('choices') and len(response_data['choices']) > 0:
                message = response_data['choices'][0].get('message', {})
                return message.get('content', '')
            else:
                return ""
                
        except Exception as e:
            raise Exception(f"Non-streaming error: {e}")
        finally:
            conn.close()

def main():
    print("=== Local LLM Chat (Enhanced Streaming) ===")
    print("Type 'exit' to quit.\n")
    
    client = StreamingChatClient(HOST, PORT)
    history = [{"role": "system", "content": "You are a helpful assistant."}]
    
    while True:
        try:
            user_input = input("You: ").strip()
            if user_input.lower() == "exit":
                break
            
            history.append({"role": "user", "content": user_input})
            
            print("Assistant: ", end="", flush=True)
            full_reply = ""
            streaming_worked = False
            
            try:
                # Attempt streaming
                for chunk in client.send_message_stream(history):
                    if chunk:
                        print(chunk, end="", flush=True)
                        full_reply += chunk
                        streaming_worked = True
                        # Small delay to make streaming visible
                        time.sleep(0.01)
                
                print()  # New line after streaming
                
            except Exception as e:
                print(f"\nStreaming failed: {e}")
                print("Falling back to non-streaming...")
                
                try:
                    full_reply = client.send_message_non_streaming(history)
                    if full_reply:
                        print(f"Assistant: {full_reply}")
                    else:
                        print("<no response>")
                except Exception as fallback_error:
                    print(f"Fallback also failed: {fallback_error}")
                    full_reply = ""
            
            if full_reply:
                history.append({"role": "assistant", "content": full_reply})
            elif not streaming_worked:
                print("<no response received>")
                
        except KeyboardInterrupt:
            print("\n\nExiting...")
            break
        except Exception as e:
            print(f"\nError: {e}")
            continue

if __name__ == "__main__":
    main()
