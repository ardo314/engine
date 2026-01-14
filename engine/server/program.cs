using NATS.Net;


await using var natsClient = new NatsClient(url: "nats://192.168.178.33:4222");

_ = Task.Run(async () =>
{
    while (true)
    {
        // Generate a random exchange rate from 1.00 to 2.00
        double value = 1 + Random.Shared.NextDouble();

        // Ensure it is 2 decimal places
        value = Math.Round(value, 2);

        // Publish it as GBPUSD
        await natsClient.PublishAsync(subject: "GBPUSD", data: value);

        // Output to console, then wait 1 second before sending another
        Console.WriteLine($"Sent GBPUSD: {value} - press ENTER to exit.");
        await Task.Delay(1000);
    }
});

Console.ReadLine();