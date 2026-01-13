using Server;

var foo = new Foo();
Console.WriteLine(foo.GetMessage());
await Task.Delay(2000);
Console.WriteLine("Server is running...");
